//! Bind an immutable Bayer asset or load error. Evaluation performs no file I/O.
use crate::{
    Error, Result,
    definition::{Metadata, Node, Outcome, Registration},
    node::{camera, data::*},
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
use std::{path::PathBuf, sync::Arc};

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
    bound: Result<Bound>,
}
struct Bound {
    desc: BayerDesc,
    mosaic: Arc<HostBuffer<f32>>,
    metadata: Arc<drip_raw::Metadata>,
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
            .ok_or_else(|| Error::Contract("select a RAW before evaluation".into()))
            .and_then(|path| {
                let raw = drip_raw::decode(path)
                    .map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))?;
                normalize(raw)
            });
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
            help: "Read a Bayer RAW, subtract black and normalize by a common white reference. Outputs Bayer camera responses and the decoded capture metadata, including native calibration. Choose a RAW file before evaluation; calibration remains approximate.",
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
            PortSpec::of::<Cpu<Bayer>, Any>("mosaic"),
            PortSpec::of::<Cpu<CaptureMetadata>, Any>("metadata"),
        ]
    }
    fn contract(&self, _: &GlobalContext, _: &[Option<Description>]) -> Result<Outcome> {
        Ok(match &self.bound {
            Ok(b) => Outcome::Known(vec![Arc::new(b.desc.clone()), Arc::new(b.metadata.clone())]),
            Err(reason) => Outcome::Unknown(reason.clone()),
        })
    }
    fn run(
        &self,
        _: &KernelContext<'_>,
        _: &[Option<(Description, Data)>],
        _: &[Description],
    ) -> Result<Vec<Data>> {
        let b = self.bound.as_ref().map_err(Clone::clone)?;
        Ok(vec![b.mosaic.clone(), Arc::new(b.metadata.clone())])
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
// denominator avoids introducing a hidden per-channel white balance; consumers
// derive gains and color calibration from the capture metadata.
fn normalize(raw: drip_raw::Raw) -> Result<Bound> {
    let m = &raw.metadata;
    let c = &m.calibration;
    let scale = 1.0 / camera::white_range(c)?;
    let width = m.sensor.width;
    let mosaic: Vec<f32> = raw
        .data
        .iter()
        .enumerate()
        .map(|(i, &v)| (v as f32 - camera::black_at(c, i / width, i % width) as f32) * scale)
        .collect();
    let extent = Extent {
        width: m.sensor.width.try_into().map_err(|_| Error::Contract("RAW too wide".into()))?,
        height: m.sensor.height.try_into().map_err(|_| Error::Contract("RAW too high".into()))?,
    };
    let desc = BayerDesc {
        extent,
        phase: camera::phase(m)?,
        response: Response { camera: Camera::of(m), balance: None },
    };
    Bayer::validate(&desc)?;
    let mosaic = mosaic.into();
    Bayer::check(&desc, &mosaic)?;
    let metadata = Arc::new(raw.metadata);
    CaptureMetadata::validate(&metadata)?;
    Ok(Bound { desc, mosaic: Arc::new(mosaic), metadata })
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
        let b = normalize(r.clone()).unwrap();
        assert_eq!(b.desc.extent, Extent { width: 3, height: 2 });
        assert_eq!(b.desc.phase, BayerPhase::Rggb);
        assert_eq!(b.desc.response.balance, None);
        assert!((b.mosaic[0] - 485. / 985.).abs() < 1e-7);
        assert!((b.mosaic[3] - 455. / 985.).abs() < 1e-7);
        assert_eq!(*b.metadata, r.metadata);
    }

    #[test]
    fn missing_calibration_keeps_metadata_and_mosaic_available() {
        let mut r = raw();
        r.metadata.calibration.as_shot = [0.; 4];
        r.metadata.calibration.xyz_to_cam = [[0.; 3]; 4];
        let b = normalize(r.clone()).unwrap();
        let mut dag = crate::graph::Dag::new();
        let id =
            dag.add(Arc::new(RawSource { params: Settings::default(), bound: Ok(b) })).unwrap();
        let eval = crate::eval::Evaluator::new(crate::runtime::RuntimeContext::host());
        let mosaic =
            eval.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), Output::new(id, 0)).unwrap();
        assert_eq!(mosaic.data.len(), 6);
        let metadata = eval
            .evaluate::<Cpu<CaptureMetadata>>(&dag, &Default::default(), Output::new(id, 1))
            .unwrap();
        assert_eq!(**metadata.data, r.metadata);
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn raw_neutral_survives_as_shot_to_reference_and_cat16() {
        use crate::{eval::Evaluator, node, node::color, runtime::RuntimeContext};
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
        let b = normalize(r).unwrap();
        let balanced = camera::matrix(&b.metadata, camera::as_shot_balance(&b.metadata).ok());
        let mapped = color::apply(&balanced.unwrap(), [0.5; 3]);
        assert!((mapped[0] - mapped[1]).abs() > 0.01); // WB is deferred to CAT16.
        let expected_y = color::apply(&color::rgb_to_xyz(color::REC2020, color::D65), mapped)[1];
        let mut dag = crate::graph::Dag::new();
        let mosaic = node::source::<Bayer>(&mut dag, b.desc, b.mosaic.to_vec()).unwrap();
        let metadata =
            node::source::<CaptureMetadata>(&mut dag, b.metadata.clone(), b.metadata).unwrap();
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
        let output = evaluator.evaluate::<Cpu<Rgb>>(&dag, &Default::default(), cat.output).unwrap();
        for pixel in output.data.iter() {
            for value in pixel {
                assert!((f64::from(*value) - expected_y).abs() < 2e-5);
            }
        }
    }
}
