//! Decode a Bayer RAW when first evaluated; the node keeps it or the error.
use crate::{
    Error, Result,
    node::{camera, data::*},
    ports::Load,
    registry::tag,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Bayer RAW, decoded when first evaluated.
    #[param(open = ["arw", "cr2", "cr3", "dng", "erf", "fff", "iiq", "kdc", "mef", "mos", "mrw", "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "rwl", "srw", "x3f"])]
    #[label("RAW file")]
    #[external]
    pub path: Option<PathBuf>,
}
/// Read a Bayer RAW, subtract black and normalize by a common white reference.
/// Outputs Bayer camera responses and the decoded capture metadata, including
/// native calibration. The file is decoded when first evaluated and kept with
/// the node; choose it again to pick up changes. Calibration remains approximate.
#[crate::node(id = "bayer-raw", name = "Bayer RAW", tags = [tag::INPUT_TRANSFORM, tag::BAYER_RAW, tag::IO], references = [(
    "LibRaw black normalization",
    "https://github.com/LibRaw/LibRaw/blob/0.22.0/src/utils/utils_libraw.cpp",
)])]
pub fn read(
    p: &Settings,
    mosaic: Load<'_, Bayer>,
    metadata: Load<'_, CaptureMetadata>,
) -> Result<()> {
    let path =
        p.path.as_ref().ok_or_else(|| Error::Contract("select a RAW before evaluation".into()))?;
    let raw =
        drip_raw::decode(path).map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))?;
    let (desc, samples) = normalize(&raw)?;
    mosaic.set(desc, samples.into())?;
    let facts = Arc::new(raw.metadata);
    metadata.set(facts.clone(), facts)
}
pub use read::{Definition, Ports, add, definition};

// Adapted from Drip b9b1045 raw normalization (LibRaw 0.22 model). The common
// denominator avoids introducing a hidden per-channel white balance; consumers
// derive gains and color calibration from the capture metadata.
fn normalize(raw: &drip_raw::Raw) -> Result<(BayerDesc, Vec<f32>)> {
    let m = &raw.metadata;
    let c = &m.calibration;
    let scale = 1.0 / camera::white_range(c)?;
    let width = m.sensor.width;
    let samples = raw
        .data
        .iter()
        .enumerate()
        .map(|(i, &v)| (v as f32 - camera::black_at(c, i / width, i % width) as f32) * scale)
        .collect();
    let extent = Extent {
        width: m.sensor.width.try_into().map_err(|_| Error::Contract("RAW too wide".into()))?,
        height: m.sensor.height.try_into().map_err(|_| Error::Contract("RAW too high".into()))?,
    };
    let response = Response { camera: Camera::of(m), balance: None };
    Ok((BayerDesc { extent, phase: camera::phase(m)?, response }, samples))
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
    fn normalization_preserves_partial_bayer_cells_and_needs_no_calibration() {
        let mut r = raw();
        let (desc, mosaic) = normalize(&r).unwrap();
        assert_eq!(desc.extent, Extent { width: 3, height: 2 });
        assert_eq!(desc.phase, BayerPhase::Rggb);
        assert_eq!(desc.response.balance, None);
        assert!((mosaic[0] - 485. / 985.).abs() < 1e-7);
        assert!((mosaic[3] - 455. / 985.).abs() < 1e-7);
        // Processing calibration is derived by consumers, not required to load.
        r.metadata.calibration.as_shot = [0.; 4];
        r.metadata.calibration.xyz_to_cam = [[0.; 3]; 4];
        assert_eq!(normalize(&r).unwrap().1, mosaic);
    }

    #[test]
    fn missing_or_unreadable_files_stay_editable_and_fail_evaluation() {
        use crate::{eval::Evaluator, ports::Cpu, runtime::RuntimeContext};
        let mut dag = crate::graph::Dag::new();
        let unset = add(&mut dag, Settings::default()).unwrap();
        let missing = Settings { path: Some("/nonexistent/drip.nef".into()) };
        let missing = add(&mut dag, missing).unwrap();
        let eval = Evaluator::new(RuntimeContext::host());
        for (ports, reason) in [(unset, "select a RAW"), (missing, "/nonexistent/drip.nef")] {
            assert!(dag.node(ports.node).unwrap().parameters().is_ok());
            let error =
                eval.evaluate::<Cpu<Bayer>>(&dag, &Default::default(), ports.mosaic).err().unwrap();
            assert!(error.to_string().contains(reason), "{error}");
        }
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn raw_neutral_survives_as_shot_to_reference_and_cat16() {
        use crate::{eval::Evaluator, node, node::color, ports::Cpu, runtime::RuntimeContext};
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
        let (desc, mosaic) = normalize(&r).unwrap();
        let facts = Arc::new(r.metadata);
        let balanced = camera::matrix(&facts, camera::as_shot_balance(&facts).ok());
        let mapped = color::apply(&balanced.unwrap(), [0.5; 3]);
        assert!((mapped[0] - mapped[1]).abs() > 0.01); // WB is deferred to CAT16.
        let expected_y = color::apply(&color::rgb_to_xyz(color::REC2020, color::D65), mapped)[1];
        let mut dag = crate::graph::Dag::new();
        let mosaic = node::source::<Bayer>(&mut dag, desc, mosaic).unwrap();
        let metadata = node::source::<CaptureMetadata>(&mut dag, facts.clone(), facts).unwrap();
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
