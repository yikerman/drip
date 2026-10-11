//! Bind an immutable Bayer asset or load error. Evaluation performs no file I/O.
use crate::{
    Error, Result,
    definition::{Metadata, Node, Registration},
    node::color,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
use std::{path::PathBuf, sync::Arc};

pub fn working_color() -> Color {
    Color {
        space: "rec2020-d65".into(),
        encoding: Encoding::Identity,
        scale: "relative-white-1".into(),
    }
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Bayer RAW to bind as an immutable decoded snapshot.
    #[param(crate::param::ParamKind::Path {output:false})]
    #[label("RAW file")]
    #[external]
    pub path: Option<PathBuf>,
}
pub struct RawSource {
    params: Settings,
    bound: Result<Option<Bound>>,
}
struct Bound {
    metadata: Arc<CaptureData>,
    desc: BayerDesc,
    mosaic: Arc<HostBuffer<f32>>,
}
impl RawSource {
    /// Snapshot a source or its load error. Failed assets remain editable and
    /// serializable; no image interpretation is claimed until binding succeeds.
    pub fn bind(mut params: Settings) -> Arc<dyn Node> {
        if let Some(path) = &params.path
            && let Ok(absolute) = path.canonicalize()
        {
            params.path = Some(absolute);
        }
        let bound = params
            .path
            .as_ref()
            .map(|path| {
                let raw = drip_raw::decode(path)
                    .map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))?;
                normalize(&raw)
            })
            .transpose();
        Arc::new(Self { params, bound })
    }
}
impl Node for RawSource {
    fn loads_assets(&self) -> bool {
        true
    }
    fn metadata(&self) -> Metadata {
        Metadata {
            id: "input.bayer-raw",
            name: "RAW",
            category: "Input",
            help: "Read a Bayer RAW, subtract black and normalize by a common white reference. Outputs are Bayer and CaptureMetadata, including common capture fields and calibration. Gains and matrix may be unavailable when camera metadata is insufficient. Choose a RAW file before evaluation; calibration remains approximate.",
            references: &[(
                "LibRaw black normalization",
                "https://github.com/LibRaw/LibRaw/blob/0.22.0/src/utils/utils_libraw.cpp",
            )],
            parameters: <Settings as crate::param::Parameters>::SPECS,
        }
    }
    fn parameters(&self) -> Result<serde_json::Value> {
        serde_json::to_value(&self.params).map_err(|e| Error::Graph(e.to_string()))
    }
    fn inputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![
            PortSpec::of::<Cpu<Bayer>>("mosaic").named_payload("Bayer"),
            PortSpec::of::<Cpu<CaptureMetadata>>("metadata").named_payload("CaptureMetadata"),
        ]
    }
    fn contract(
        &self,
        _: &GlobalContext,
        _: &[Option<Description>],
    ) -> Result<Vec<Option<Description>>> {
        let Ok(Some(b)) = &self.bound else {
            return Ok(vec![None, None]);
        };
        Ok(vec![
            erase::<Bayer>(Some(b.desc.clone()))?,
            erase::<CaptureMetadata>(Some(MetadataDesc::of(&b.metadata)))?,
        ])
    }
    fn run(
        &self,
        _: &KernelContext<'_>,
        _: &[Option<Description>],
        _: &[Option<Data>],
        _: &[Option<Description>],
    ) -> Result<Vec<Option<Data>>> {
        let b = self
            .bound
            .as_ref()
            .map_err(Clone::clone)?
            .as_ref()
            .ok_or_else(|| Error::Contract("select a RAW before evaluation".into()))?;
        Ok(vec![Some(b.mosaic.clone()), Some(b.metadata.clone())])
    }
}
#[linkme::distributed_slice(crate::definition::NODES)]
static REGISTRATION: Registration = Registration {
    id: "input.bayer-raw",
    defaults: || serde_json::json!({"path":null}),
    build: |p| {
        Ok(RawSource::bind(serde_json::from_value(p).map_err(|e| Error::Contract(e.to_string()))?))
    },
};

// Adapted from Drip b9b1045 raw normalization (LibRaw 0.22 model). The common
// denominator avoids introducing a hidden per-channel white balance. Gains and
// the color matrix are explicit calibration within the capture metadata.
fn normalize(raw: &drip_raw::Raw) -> Result<Bound> {
    let pattern = raw.metadata.calibration.cfa.map(|r| r.map(|c| if c == 3 { 1 } else { c }));
    let phase = match pattern {
        [[0, 1], [1, 2]] => BayerPhase::Rggb,
        [[1, 0], [2, 1]] => BayerPhase::Grbg,
        [[1, 2], [0, 1]] => BayerPhase::Gbrg,
        [[2, 1], [1, 0]] => BayerPhase::Bggr,
        _ => return Err(Error::Contract("source is not a supported Bayer pattern".into())),
    };
    let mosaic = normalized_samples(raw)?.into();
    let gains = site_gains(raw);
    let xyz_to_cam = std::array::from_fn(|i| raw.metadata.calibration.xyz_to_cam[i]);
    let matrix = reference_matrix(&xyz_to_cam, raw.metadata.calibration.as_shot);
    let camera = Camera {
        coordinates: format!(
            "{}/{}:{:?}",
            std::str::from_utf8(&raw.metadata.make).expect("decoder returns UTF-8"),
            std::str::from_utf8(&raw.metadata.model).expect("decoder returns UTF-8"),
            xyz_to_cam,
        ),
        scale: "black-relative-white-reference".into(),
    };
    let balanced = gains.map(|gains| Camera {
        coordinates: format!("{}; site-gains={gains:?}", camera.coordinates),
        scale: camera.scale.clone(),
    });
    let desc = BayerDesc {
        extent: Extent {
            width: raw
                .metadata
                .sensor
                .width
                .try_into()
                .map_err(|_| Error::Contract("RAW too wide".into()))?,
            height: raw
                .metadata
                .sensor
                .height
                .try_into()
                .map_err(|_| Error::Contract("RAW too high".into()))?,
        },
        phase,
        interpretation: camera.clone(),
    };
    Bayer::validate_cpu(&desc, &mosaic)?;
    // Four-site approximation at the first Bayer cell; larger black grids
    // require a spatial level field to describe clipping exactly.
    let levels = std::array::from_fn(|i| {
        (raw.metadata.calibration.maximum as f32 - black_at(raw, i / 2, i % 2) as f32)
            / (raw.metadata.calibration.maximum as f32 - black_reference(raw) as f32)
    });
    let matrix = balanced.as_ref().zip(matrix).map(|(source, values)| CameraCalibration {
        source: source.clone(),
        target: working_color(),
        values: color::to_f32(&values),
    });
    let metadata = CaptureData {
        capture: raw.metadata.clone(),
        bayer: Some(BayerCalibration {
            phase,
            source: camera,
            levels,
            gains: balanced.zip(gains).map(|(target, values)| SiteGains { target, values }),
        }),
        matrix,
    };
    Ok(Bound { metadata: Arc::new(metadata), desc, mosaic: Arc::new(mosaic) })
}

// Keep as-shot gains through highlights/demosaic, then replace them with D65
// reference gains in the camera matrix. This is darktable's "as shot to
// reference" placement [22], without another image pass. Scene WB follows in CAT16.
fn reference_matrix(xyz_to_cam: &[[f32; 3]; 3], as_shot: [f32; 4]) -> Option<color::Mat3> {
    let xyz_to_cam = color::to_f64(xyz_to_cam);
    let rgb_to_xyz = color::rgb_to_xyz(color::REC2020, color::D65);
    let reference = color::apply(&xyz_to_cam, color::apply(&rgb_to_xyz, [1.; 3]));
    if reference.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || as_shot[..3].iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return None;
    }
    let correction: [f64; 3] = std::array::from_fn(|c| {
        (reference[1] / reference[c]) * f64::from(as_shot[1]) / f64::from(as_shot[c])
    });
    let matrix = color::camera_to_rgb(&xyz_to_cam, &rgb_to_xyz)?
        .map(|row| std::array::from_fn(|c| row[c] * correction[c]));
    matrix.iter().flatten().all(|v| (*v as f32).is_finite()).then_some(matrix)
}

fn normalized_samples(raw: &drip_raw::Raw) -> Result<Vec<f32>> {
    let common = black_reference(raw);
    if u64::from(raw.metadata.calibration.maximum) <= common {
        return Err(Error::Contract("white reference is not above black".into()));
    }
    let scale = 1.0 / (u64::from(raw.metadata.calibration.maximum) - common) as f32;
    Ok(raw
        .data
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            (v as f32
                - black_at(raw, i / raw.metadata.sensor.width, i % raw.metadata.sensor.width)
                    as f32)
                * scale
        })
        .collect())
}
fn site_gains(raw: &drip_raw::Raw) -> Option<[f32; 4]> {
    let mut wb = raw.metadata.calibration.as_shot;
    // LibRaw uses zero for an unspecified second green. This explicit decoder
    // convention does not choose or infer a demosaic or white-balance algorithm.
    if wb[3] <= 0.0 {
        wb[3] = wb[1];
    }
    let gains =
        std::array::from_fn(|i| wb[raw.metadata.calibration.cfa[i / 2][i % 2] as usize] / wb[1]);
    gains.iter().all(|v| v.is_finite() && *v > 0.0).then_some(gains)
}

fn black_at(raw: &drip_raw::Raw, r: usize, c: usize) -> u64 {
    u64::from(raw.metadata.calibration.black)
        + u64::from(
            raw.metadata.calibration.channel_black
                [raw.metadata.calibration.cfa[r % 2][c % 2] as usize],
        )
        + u64::from(raw.metadata.calibration.pattern.at(r, c))
}
fn black_reference(raw: &drip_raw::Raw) -> u64 {
    if (1..=2).contains(&raw.metadata.calibration.pattern.height)
        && (1..=2).contains(&raw.metadata.calibration.pattern.width)
    {
        (0..4).map(|i| black_at(raw, i / 2, i % 2)).min().unwrap()
    } else {
        u64::from(raw.metadata.calibration.black)
            + u64::from(*raw.metadata.calibration.channel_black.iter().min().unwrap())
            + u64::from(raw.metadata.calibration.pattern.values.iter().copied().min().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw() -> drip_raw::Raw {
        drip_raw::Raw {
            data: vec![600; 6],
            metadata: drip_raw::Metadata {
                sensor: drip_raw::Sensor { width: 3, height: 2, ..Default::default() },
                calibration: drip_raw::Calibration {
                    cfa: [[0, 1], [3, 2]],
                    black: 100,
                    channel_black: [10, 20, 30, 40],
                    pattern: drip_raw::BlackPattern { width: 2, height: 1, values: vec![5, 7] },
                    maximum: 1100,
                    xyz_to_cam: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]],
                    as_shot: [2., 1., 1.5, 0.],
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }
    #[test]
    fn normalization_preserves_capture_and_partial_bayer_cells() {
        let r = raw();
        let b = normalize(&r).unwrap();
        assert_eq!(b.desc.extent, Extent { width: 3, height: 2 });
        assert!((b.mosaic[0] - 485. / 985.).abs() < 1e-7);
        assert!((b.mosaic[3] - 455. / 985.).abs() < 1e-7);
        let calibration = b.metadata.bayer.as_ref().unwrap();
        assert_eq!(calibration.gains.as_ref().unwrap().values, [2., 1., 1., 1.5]);
        assert_eq!(calibration.levels, [1., 973. / 985., 955. / 985., 963. / 985.]);
        assert_eq!(b.metadata.capture, r.metadata);
        assert_eq!(
            b.metadata.matrix.as_ref().unwrap().source,
            calibration.gains.as_ref().unwrap().target
        );
    }
    #[test]
    fn missing_processing_calibration_keeps_metadata_and_mosaic_available() {
        let mut r = raw();
        r.metadata.calibration.as_shot = [0.; 4];
        r.metadata.calibration.xyz_to_cam = [[0.; 3]; 4];
        let b = normalize(&r).unwrap();
        assert!(b.metadata.bayer.as_ref().unwrap().gains.is_none());
        assert!(b.metadata.matrix.is_none());
        let mut dag = crate::graph::Dag::new();
        let id = dag
            .add(Arc::new(RawSource { params: Settings::default(), bound: Ok(Some(b)) }))
            .unwrap();
        let eval = crate::eval::Evaluator::new(crate::runtime::RuntimeContext::host());
        let mosaic =
            eval.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), Output::new(id, 0)).unwrap();
        assert_eq!(mosaic.data.len(), 6);
        let metadata = eval
            .evaluate::<Cpu<CaptureMetadata>>(&dag, &Default::default(), Output::new(id, 1))
            .unwrap();
        assert_eq!(metadata.data.capture, r.metadata);
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn raw_neutral_survives_as_shot_to_reference_and_cat16() {
        use crate::{eval::Evaluator, node, runtime::RuntimeContext};
        let mut r = raw();
        r.metadata.sensor.width = 64;
        r.metadata.sensor.height = 64;
        r.metadata.calibration.black = 0;
        r.metadata.calibration.channel_black = [0; 4];
        r.metadata.calibration.pattern = Default::default();
        r.metadata.calibration.maximum = 10000;
        r.metadata.calibration.xyz_to_cam =
            [[0.6, 0.2, 0.05], [0.1, 0.8, 0.1], [0.02, 0.1, 0.7], [0.; 3]];
        r.metadata.calibration.as_shot = [2., 1., 4., 0.];
        r.data = (0..64 * 64)
            .map(|i| {
                [2500, 5000, 1250, 5000][r.metadata.calibration.cfa[i / 64 % 2][i % 2] as usize]
            })
            .collect();
        let b = normalize(&r).unwrap();
        let camera_matrix = &b.metadata.matrix.as_ref().unwrap().values;
        let mapped = color::apply(&color::to_f64(camera_matrix), [0.5; 3]);
        assert!((mapped[0] - mapped[1]).abs() > 0.01); // WB is now deferred to CAT16.
        let expected_y = color::apply(&color::rgb_to_xyz(color::REC2020, color::D65), mapped)[1];
        let mut dag = crate::graph::Dag::new();
        let mosaic = node::source::<Bayer>(&mut dag, b.desc, b.mosaic.to_vec()).unwrap();
        let metadata = node::source::<CaptureMetadata>(
            &mut dag,
            MetadataDesc::of(&b.metadata),
            (*b.metadata).clone(),
        )
        .unwrap();
        let gains = node::bayer_gains::add(&mut dag, ()).unwrap();
        let highlights = node::highlights::reconstruct::add(&mut dag, Default::default()).unwrap();
        let demosaic = node::rcd::add(&mut dag, ()).unwrap();
        let color = node::camera_to_rgb::add(&mut dag, ()).unwrap();
        let cat = node::chromatic_adaptation::add(&mut dag, Default::default()).unwrap();
        dag.connect(mosaic, gains.image).unwrap();
        dag.connect(metadata, gains.metadata).unwrap();
        dag.connect(gains.output, highlights.image).unwrap();
        dag.connect(gains.levels, highlights.levels).unwrap();
        dag.connect(highlights.output, demosaic.image).unwrap();
        dag.connect(demosaic.output, color.image).unwrap();
        dag.connect(metadata, color.metadata).unwrap();
        dag.connect(color.output, cat.image).unwrap();
        dag.connect(metadata, cat.metadata).unwrap();
        let evaluator = Evaluator::new(RuntimeContext::cpu());
        let levels = evaluator
            .evaluate::<Cpu<BayerLevels>>(&dag, &Default::default(), gains.levels)
            .unwrap();
        assert_eq!(*levels.data, [2., 1., 1., 4.]);
        let output =
            evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &Default::default(), cat.output).unwrap();
        for pixel in output.data.iter() {
            for value in pixel {
                assert!((f64::from(*value) - expected_y).abs() < 2e-5);
            }
        }
    }

    #[test]
    fn camera_conversion_cancels_conditioning_gains() {
        // An ideal XYZ camera must recover the original tristimulus values,
        // regardless of the as-shot gains used to condition Bayer processing.
        let xyz_camera = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let rgb_to_xyz = color::rgb_to_xyz(color::REC2020, color::D65);
        let native = [0.2, 0.3, 0.4];
        for gains in [[1., 1., 1., 0.], [2., 1., 4., 0.], [4., 2., 3., 0.]] {
            let balanced =
                std::array::from_fn(|c| native[c] * f64::from(gains[c]) / f64::from(gains[1]));
            let matrix = reference_matrix(&xyz_camera, gains).unwrap();
            let recovered = color::apply(&rgb_to_xyz, color::apply(&matrix, balanced));
            for (actual, expected) in recovered.into_iter().zip(native) {
                assert!((actual - expected).abs() < 1e-12);
            }
        }
    }
}
