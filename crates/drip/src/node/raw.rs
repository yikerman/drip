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
pub type CameraMatrix = Matrix3<Camera, Color>;
pub struct RawSource {
    params: Settings,
    bound: Result<Option<Bound>>,
}
struct Bound {
    levels: Arc<[f32; 4]>,
    metadata: Arc<CaptureData>,
    desc: BayerDesc,
    mosaic: Arc<HostBuffer<f32>>,
    gains: Option<Arc<[f32; 4]>>,
    gain_desc: Option<BayerGainDesc>,
    matrix: Option<Arc<[[f32; 3]; 3]>>,
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
            help: "Read a Bayer RAW, subtract black and normalize by a common white reference. Outputs are Bayer, BayerGains, CameraMatrix, BayerLevels and CaptureMetadata. Gains and matrix may be unavailable when camera metadata is insufficient. Choose a RAW file before evaluation; calibration remains approximate.",
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
            PortSpec::of::<Cpu<BayerGains>>("gains").named_payload("BayerGains"),
            PortSpec::of::<Cpu<CameraMatrix>>("matrix").named_payload("CameraMatrix"),
            PortSpec::of::<Cpu<CaptureMetadata>>("metadata").named_payload("CaptureMetadata"),
            PortSpec::of::<Cpu<BayerLevels>>("levels").named_payload("BayerLevels"),
        ]
    }
    fn contract(
        &self,
        _: &GlobalContext,
        _: &[Option<Description>],
    ) -> Result<Vec<Option<Description>>> {
        let Ok(Some(b)) = &self.bound else {
            return Ok(vec![None, None, None, None, None]);
        };
        Ok(vec![
            erase::<Bayer>(Some(b.desc.clone()))?,
            erase::<BayerGains>(b.gain_desc.clone())?,
            erase::<CameraMatrix>(
                b.matrix
                    .as_ref()
                    .and(b.gain_desc.as_ref())
                    .map(|g| MatrixDesc { source: g.target.clone(), target: working_color() }),
            )?,
            erase::<CaptureMetadata>(Some(MetadataDesc::of(&b.metadata)))?,
            erase::<BayerLevels>(Some(BayerLevelDesc {
                phase: b.desc.phase,
                interpretation: b.desc.interpretation.clone(),
            }))?,
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
        Ok(vec![
            Some(b.mosaic.clone()),
            b.gains.clone().map(|v| v as Data),
            b.matrix.clone().map(|v| v as Data),
            Some(b.metadata.clone()),
            Some(b.levels.clone()),
        ])
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
// the color matrix are now independent data products, not image metadata.
fn normalize(raw: &drip_raw::Raw) -> Result<Bound> {
    let pattern = raw.cfa.map(|r| r.map(|c| if c == 3 { 1 } else { c }));
    let phase = match pattern {
        [[0, 1], [1, 2]] => BayerPhase::Rggb,
        [[1, 0], [2, 1]] => BayerPhase::Grbg,
        [[1, 2], [0, 1]] => BayerPhase::Gbrg,
        [[2, 1], [1, 0]] => BayerPhase::Bggr,
        _ => return Err(Error::Contract("source is not a supported Bayer pattern".into())),
    };
    let mosaic = normalized_samples(raw)?.into();
    let gains = site_gains(raw);
    let matrix = color::camera_to_rgb(
        &color::to_f64(&raw.xyz_to_cam),
        &color::rgb_to_xyz(color::REC2020, color::D65),
    )
    .filter(|m| m.iter().flatten().all(|v| v.is_finite()));
    let camera = Camera {
        coordinates: format!(
            "{}/{}:{:?}",
            std::str::from_utf8(&raw.metadata.make).expect("decoder returns UTF-8"),
            std::str::from_utf8(&raw.metadata.model).expect("decoder returns UTF-8"),
            raw.xyz_to_cam,
        ),
        scale: "black-relative-white-reference".into(),
    };
    let balanced = gains.map(|gains| Camera {
        coordinates: format!("{}; site-gains={gains:?}", camera.coordinates),
        scale: camera.scale.clone(),
    });
    let desc = BayerDesc {
        extent: Extent {
            width: raw.width.try_into().map_err(|_| Error::Contract("RAW too wide".into()))?,
            height: raw.height.try_into().map_err(|_| Error::Contract("RAW too high".into()))?,
        },
        phase,
        interpretation: camera.clone(),
    };
    Bayer::validate_cpu(&desc, &mosaic)?;
    Ok(Bound {
        metadata: Arc::new(raw.metadata.clone()),
        // The four-site clipping approximation uses the first local Bayer cell.
        // Larger black-level grids can vary by location; exact clipping would
        // need a spatial level field rather than four scalar values.
        levels: Arc::new(std::array::from_fn(|i| {
            (raw.maximum as f32 - black_at(raw, i / 2, i % 2) as f32)
                / (raw.maximum as f32 - black_reference(raw) as f32)
        })),
        desc,
        mosaic: Arc::new(mosaic),
        gains: gains.map(Arc::new),
        gain_desc: balanced.map(|target| BayerGainDesc { phase, source: camera, target }),
        matrix: gains.and(matrix).map(|m| Arc::new(color::to_f32(&m))),
    })
}

fn normalized_samples(raw: &drip_raw::Raw) -> Result<Vec<f32>> {
    let common = black_reference(raw);
    if u64::from(raw.maximum) <= common {
        return Err(Error::Contract("white reference is not above black".into()));
    }
    let scale = 1.0 / (u64::from(raw.maximum) - common) as f32;
    Ok(raw
        .data
        .iter()
        .enumerate()
        .map(|(i, &v)| (v as f32 - black_at(raw, i / raw.width, i % raw.width) as f32) * scale)
        .collect())
}
fn site_gains(raw: &drip_raw::Raw) -> Option<[f32; 4]> {
    let mut wb = raw.as_shot;
    // LibRaw uses zero for an unspecified second green. This explicit decoder
    // convention does not choose or infer a demosaic or white-balance algorithm.
    if wb[3] <= 0.0 {
        wb[3] = wb[1];
    }
    let gains = std::array::from_fn(|i| wb[raw.cfa[i / 2][i % 2] as usize] / wb[1]);
    gains.iter().all(|v| v.is_finite() && *v > 0.0).then_some(gains)
}

fn black_at(raw: &drip_raw::Raw, r: usize, c: usize) -> u64 {
    u64::from(raw.black)
        + u64::from(raw.channel_black[raw.cfa[r % 2][c % 2] as usize])
        + u64::from(raw.pattern.at(r, c))
}
fn black_reference(raw: &drip_raw::Raw) -> u64 {
    if (1..=2).contains(&raw.pattern.height) && (1..=2).contains(&raw.pattern.width) {
        (0..4).map(|i| black_at(raw, i / 2, i % 2)).min().unwrap()
    } else {
        u64::from(raw.black)
            + u64::from(*raw.channel_black.iter().min().unwrap())
            + u64::from(raw.pattern.values.iter().copied().min().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw() -> drip_raw::Raw {
        drip_raw::Raw {
            width: 3,
            height: 2,
            data: vec![600; 6],
            cfa: [[0, 1], [3, 2]],
            black: 100,
            channel_black: [10, 20, 30, 40],
            pattern: drip_raw::BlackPattern { width: 2, height: 1, values: vec![5, 7] },
            maximum: 1100,
            xyz_to_cam: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot: [2., 1., 1.5, 0.],
            metadata: Default::default(),
        }
    }
    fn inspect_contract<P>(_: &GlobalContext, _: &(), _: Option<&P>) -> Result<()> {
        Ok(())
    }
    #[crate::node(id="test-inspect-mosaic", contract=inspect_contract)]
    fn inspect_mosaic(_: &KernelContext<'_>, _: &(), image: Read<'_, Cpu<Bayer>>) -> Result<()> {
        assert_eq!(image.data.len(), 6);
        Ok(())
    }
    #[crate::node(id="test-inspect-gains", contract=inspect_contract)]
    fn inspect_gains(
        _: &KernelContext<'_>,
        _: &(),
        _gains: Read<'_, Cpu<BayerGains>>,
    ) -> Result<()> {
        Ok(())
    }

    #[test]
    fn normalization_keeps_partial_cells_and_independent_calibration() {
        let b = normalize(&raw()).unwrap();
        assert_eq!(b.desc.extent, Extent { width: 3, height: 2 });
        assert!((b.mosaic[0] - 485. / 985.).abs() < 1e-7);
        assert!((b.mosaic[3] - 455. / 985.).abs() < 1e-7);
        assert_eq!(**b.gains.as_ref().unwrap(), [2., 1., 1., 1.5]);
        let mut r = raw();
        r.as_shot = [0.; 4];
        r.xyz_to_cam = [[0.; 3]; 3];
        let b = normalize(&r).unwrap();
        assert!(b.gains.is_none() && b.matrix.is_none());
        let mut dag = crate::graph::Dag::new();
        let id = dag
            .add(Arc::new(RawSource { params: Settings::default(), bound: Ok(Some(b)) }))
            .unwrap();
        let result = crate::eval::Evaluator::new(crate::runtime::RuntimeContext::host())
            .evaluate::<Cpu<Bayer>>(&dag, &Default::default(), Output::new(id, 0))
            .unwrap();
        assert_eq!(result.data.len(), 6);
        let gains = super::super::bayer_gains::add(&mut dag, ()).unwrap();
        dag.connect(Output::new(id, 1), gains.gains).unwrap();
        assert!(
            crate::eval::Evaluator::new(crate::runtime::RuntimeContext::host())
                .evaluate_node(&dag, &Default::default(), gains.output.node())
                .is_err()
        );
        let image = inspect_mosaic::add(&mut dag, ()).unwrap();
        let calibration = inspect_gains::add(&mut dag, ()).unwrap();
        dag.connect(Output::new(id, 0), image.image).unwrap();
        dag.connect(Output::new(id, 1), calibration._gains).unwrap();
        let values = crate::eval::Evaluator::new(crate::runtime::RuntimeContext::host())
            .evaluate_inputs(&dag, &Default::default(), &[image.node, calibration.node]);
        assert!(values[&image.node].is_ok());
        assert!(values[&calibration.node].is_err());
    }
}
