//! A synthetic calibration DAG: two decoded planes produce a corrected plane;
//! a separate chart branch produces a reusable gain calibration. No node takes
//! paths except a reader would. Run with `cargo run -p drip --example calibration_dag`.
//!
//! This models sensor-grid correspondence, not a production RAW calibration:
//! samples use common black-subtracted units and carry no clipping/noise claims.
use drip::eval::Evaluator;
use drip::graph::{Graph, Port};
use drip::node::KernelError;
use drip::param::ParamKind;
use drip::value::EdgeValue;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SensorPlane {
    samples: Arc<[f32]>,
    origin: i64,
}
impl EdgeValue for SensorPlane {
    const NAME: &'static str = "Sensor plane";
}

#[derive(Debug)]
pub struct GainCalibration(f32);
impl EdgeValue for GainCalibration {
    const NAME: &'static str = "Gain calibration";
}

/// A batch is an ordinary payload; the executor need not understand frames.
#[derive(Debug)]
pub struct SensorFrames(Vec<Arc<SensorPlane>>);
impl EdgeValue for SensorFrames {
    const NAME: &'static str = "Sensor frames";
}

#[drip::node(kind = PAIR, id = "demo.pair", category = "demo", name = "Collect frames", outputs = ["frames"])]
fn pair(a: &SensorPlane, b: &SensorPlane) -> Result<(Arc<SensorFrames>,), KernelError> {
    Ok((Arc::new(SensorFrames(vec![Arc::new(a.clone()), Arc::new(b.clone())])),))
}

/// Arithmetic mean at corresponding sites; no exposure normalization or registration.
#[drip::node(kind = AVERAGE, id = "demo.average", category = "demo", name = "Average frames", outputs = ["plane"])]
fn average(frames: &SensorFrames) -> Result<(Arc<SensorPlane>,), KernelError> {
    let first = frames.0.first().ok_or("empty frame collection")?;
    let mut samples = vec![0.0; first.samples.len()];
    for frame in &frames.0 {
        matching_grid(first, frame)?;
        for (sum, sample) in samples.iter_mut().zip(frame.samples.iter()) {
            *sum += sample / frames.0.len() as f32;
        }
    }
    Ok((Arc::new(SensorPlane { samples: samples.into(), origin: first.origin }),))
}

#[derive(drip::Parameters)]
pub struct Source {
    #[param(ParamKind::Float { min: 0.0, max: 100.0, default: 10.0 })]
    value: f32,
    #[param(ParamKind::Int { min: 0, max: 100, default: 0 })]
    origin: i64,
}

/// Generate four sensor sites in common black-subtracted signal units.
#[drip::node(kind = SOURCE, id = "demo.source", category = "demo", name = "Source", outputs = ["plane"])]
fn source(#[params] p: Source) -> Result<(Arc<SensorPlane>,), KernelError> {
    Ok((Arc::new(SensorPlane { samples: vec![p.value; 4].into(), origin: p.origin }),))
}

fn matching_grid(image: &SensorPlane, dark: &SensorPlane) -> Result<(), String> {
    if image.origin != dark.origin || image.samples.len() != dark.samples.len() {
        return Err("image and dark use different sensor sites".into());
    }
    Ok(())
}

/// Subtract a dark plane in the same units at corresponding sensor sites.
/// Negative results remain representable.
#[drip::node(kind = SUBTRACT, id = "demo.subtract", category = "demo", name = "Subtract dark", outputs = ["plane"])]
fn subtract(image: &SensorPlane, dark: &SensorPlane) -> Result<(Arc<SensorPlane>,), KernelError> {
    matching_grid(image, dark)?;
    let samples: Vec<_> =
        image.samples.iter().zip(dark.samples.iter()).map(|(a, b)| a - b).collect();
    Ok((Arc::new(SensorPlane { samples: samples.into(), origin: image.origin }),))
}

/// Fit a scalar gain mapping the measured neutral patch mean to one.
/// This deliberately simple fit illustrates a reusable calibration value.
#[drip::node(kind = FIT, id = "demo.fit", category = "demo", name = "Fit neutral", outputs = ["calibration"])]
fn fit(chart: &SensorPlane) -> Result<(Arc<GainCalibration>,), KernelError> {
    let mean = chart.samples.iter().sum::<f32>() / chart.samples.len() as f32;
    if !mean.is_finite() || mean <= 0.0 {
        return Err("chart neutral must have a positive finite mean".into());
    }
    Ok((Arc::new(GainCalibration(mean.recip())),))
}

/// Apply the fitted gain to another plane in the same signal units.
/// The chart and subject need not have matching sensor coordinates.
#[drip::node(kind = APPLY, id = "demo.apply", category = "demo", name = "Apply gain", outputs = ["plane"])]
fn apply(
    image: &SensorPlane,
    calibration: &GainCalibration,
) -> Result<(Arc<SensorPlane>,), KernelError> {
    Ok((Arc::new(SensorPlane {
        samples: image.samples.iter().map(|v| v * calibration.0).collect(),
        origin: image.origin,
    }),))
}

pub fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = Graph::default();
    let light = graph.add_node(&SOURCE);
    let dark = graph.add_node(&SOURCE);
    let second_dark = graph.add_node(&SOURCE);
    let chart = graph.add_node(&SOURCE);
    graph.set_param(dark, "value", 2.0.into())?;
    graph.set_param(second_dark, "value", 4.0.into())?;
    graph.set_param(chart, "value", 4.0.into())?;
    graph.set_param(chart, "origin", 20.into())?;
    let subtract = graph.add_node(&SUBTRACT);
    let fit = graph.add_node(&FIT);
    let apply = graph.add_node(&APPLY);
    let plane = |id| Port(id, "plane".into());
    let collect = graph.add_node(&PAIR);
    let master = graph.add_node(&AVERAGE);
    graph.connect(plane(dark), Port(collect, "a".into()))?;
    graph.connect(plane(second_dark), Port(collect, "b".into()))?;
    graph.connect(Port(collect, "frames".into()), Port(master, "frames".into()))?;
    graph.connect(plane(light), Port(subtract, "image".into()))?;
    graph.connect(plane(master), Port(subtract, "dark".into()))?;
    graph.connect(plane(chart), Port(fit, "chart".into()))?;
    graph.connect(plane(subtract), Port(apply, "image".into()))?;
    graph.connect(Port(fit, "calibration".into()), Port(apply, "calibration".into()))?;

    // A wrong payload is rejected without disturbing the existing connection.
    assert!(graph.connect(plane(chart), Port(apply, "calibration".into())).is_err());
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[apply]);
    let value = &evaluator.result(apply).unwrap().as_ref().unwrap()[0];
    assert_eq!(value.downcast_ref::<SensorPlane>().unwrap().samples.as_ref(), &[1.75; 4]);
    evaluator.evaluate(&graph, 0, &[apply]);

    // Same Rust type, incompatible actual sensor grid: no downstream calculation.
    graph.set_param(light, "origin", 1.into())?;
    evaluator.evaluate(&graph, 0, &[apply]);
    assert!(evaluator.result(apply).unwrap().is_err());
    graph.set_param(light, "origin", 0.into())?;
    evaluator.evaluate(&graph, 0, &[apply]);
    assert!(evaluator.result(apply).unwrap().is_ok());
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    demo()?;
    println!(
        "Calibration DAG: (10 - mean(2, 4)) × (1 / 4) = 1.75; type and sensor-grid checks passed."
    );
    Ok(())
}

#[test]
fn calibration_branches_share_values_and_check_local_contracts() {
    demo().unwrap();
}
