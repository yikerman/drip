//! Run a real RAW pipeline through resident WGSL kernels and one CPU consumer.
//!
//! `cargo run -p drip --example gpu_pipeline -- RAW [preview-level]`
//!
//! This is a correctness/residency demo, not a microbenchmark. Decoding produces
//! a sensor mosaic on the host. White balance, reconstruction, demosaic, camera
//! conversion, exposure and rendering stay on the GPU. Only the requested
//! consumer downloads the final represented colors; sigmoid does not preserve
//! a measurement relationship to the original scene.

use drip::eval::{Evaluator, Request};
use drip::{compute::Compute, nodes, templates};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let raw = args.next().map(PathBuf::from).ok_or("usage: gpu_pipeline RAW [preview-level]")?;
    let level = args.next().map(|s| s.to_string_lossy().parse::<u8>()).transpose()?.unwrap_or(2);
    let mut project = templates::raw_to_tiff();
    let read = project.graph.nodes().find(|(_, n)| n.kind.id == nodes::READ.id).unwrap().0;
    let consumer = project.graph.nodes().find(|(_, n)| n.kind.id == nodes::TIFF.id).unwrap().0;
    project.graph.set_param(read, "path", serde_json::json!(raw))?;
    let compute = Compute::new()?;
    let evaluator = Evaluator::with_compute(compute.clone());
    let report = evaluator.request(&project.graph, level, &[Request::Inputs(consumer)]);
    if let Some((node, error)) = report.failures().next() {
        return Err(format!("node {node:?}: {error}").into());
    }
    report.with_inputs(consumer, &nodes::TIFF, |_, (image, _), _| {
        image.require_additive_color()?;
        if !image.rgb().pixels.iter().flatten().all(|v| v.is_finite()) {
            return Err("rendered image contains non-finite samples".into());
        }
        let mean: f64 = image.rgb().pixels.iter().flatten().map(|&v| f64::from(v)).sum::<f64>()
            / (image.width() * image.height() * 3) as f64;
        println!(
            "{} × {}, scale {}, mean channel {:.6}",
            image.width(),
            image.height(),
            image.scale(),
            mean
        );
        println!("meaning: {:?}", image.interpretation());
        Ok(())
    })?;
    assert_eq!(report.transfers, 2, "one upload and one final readback");
    println!("{} nodes; {} image transfers", report.executed.len(), report.transfers);
    Ok(())
}
