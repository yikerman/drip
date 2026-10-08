// Common driver for the pre-rewrite Rayon and current wgpu implementations.
// Compiled as a temporary example in each checkout; see run.py.
use drip::{
    graph::Port,
    image::{Mosaic, Rgb},
    node::{NodeDeclaration, NodeKind},
    nodes, templates,
};
use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

static INPUT: OnceLock<Arc<Mosaic>> = OnceLock::new();
struct Source;
impl NodeDeclaration for Source {
    type Parameters = ();
    type Inputs = ();
    type Outputs = (Arc<Mosaic>,);
    const KERNEL: Option<drip::node::Kernel<Self>> =
        Some(|_, (), _| Ok((INPUT.get().unwrap().clone(),)));
}
static SOURCE: NodeKind =
    NodeKind::new::<Source>("bench.source", "bench", "Preloaded RAW", &[], &["mosaic"]);

fn emit(scenario: &str, level: u8, iteration: usize, ms: f64, executed: usize) {
    println!("{},{scenario},{level},{iteration},{ms:.6},{executed}", backend::NAME);
}
fn reset(p: &mut drip::project::Project) {
    for (name, key, value) in
        [("Highlights", "threshold", 0.98), ("Exposure", "ev", 0.0), ("Sigmoid", "contrast", 1.5)]
    {
        p.graph.set_param(p.graph.find(name).unwrap(), key, serde_json::json!(value)).unwrap();
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let level: u8 = args[2].parse()?;
    let repetitions: usize = args[3].parse()?;
    let host = args[4] == "host";
    let started = Instant::now();
    let raw = drip_raw::decode(std::path::Path::new(&args[1]))?;
    let mosaic = nodes::normalize(&raw)?;
    drop(raw);
    INPUT.set(Arc::new(mosaic)).ok().unwrap();
    emit("decode_normalize", level, 0, started.elapsed().as_secs_f64() * 1000.0, 0);
    let mut p = templates::raw_to_tiff();
    let raw = p.graph.find("RAW").unwrap();
    p.graph.remove_node(raw);
    let source = p.graph.add_node(&SOURCE);
    let wb = p.graph.find("White balance").unwrap();
    p.graph.connect(Port(source, "mosaic".into()), Port(wb, "mosaic".into()))?;
    let target = p.graph.find("Sigmoid").unwrap();
    let started = Instant::now();
    let mut runner = backend::Runner::new();
    emit("runtime_init", level, 0, started.elapsed().as_secs_f64() * 1000.0, 0);
    let started = Instant::now();
    let executed = runner.run(&p, level, target, true, host);
    emit("first_evaluation", level, 0, started.elapsed().as_secs_f64() * 1000.0, executed);
    // Untimed numerical audit: whole-image mean/finite count and deterministic
    // spatial samples for cross-revision comparison. Never time checksum work.
    let image: Arc<Rgb> = runner.output(target);
    let mean = image.pixels.iter().flatten().map(|&v| f64::from(v)).sum::<f64>()
        / (image.pixels.len() * 3) as f64;
    assert!(image.pixels.iter().flatten().all(|v| v.is_finite()));
    eprintln!(
        "image={}x{} scale={} mean={mean:.10} host={host}",
        image.width, image.height, image.scale
    );
    if let Some(path) = args.get(5) {
        use std::io::Write;
        let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
        for pixel in image.pixels.iter().step_by((image.pixels.len() / 16384).max(1)) {
            for value in pixel {
                out.write_all(&value.to_le_bytes())?;
            }
        }
    }
    drop(image);
    for scenario in ["fresh", "highlight", "exposure", "sigmoid", "noop"] {
        reset(&mut p);
        for _ in 0..2 {
            runner.run(&p, level, target, scenario != "fresh", host);
        }
        for i in 0..repetitions {
            let edit = match scenario {
                "highlight" => {
                    Some(("Highlights", "threshold", if i % 2 == 0 { 0.97 } else { 0.98 }))
                }
                "exposure" => Some(("Exposure", "ev", if i % 2 == 0 { 0.25 } else { 0.5 })),
                "sigmoid" => Some(("Sigmoid", "contrast", if i % 2 == 0 { 1.4 } else { 1.6 })),
                _ => None,
            };
            if let Some((name, key, v)) = edit {
                p.graph.set_param(p.graph.find(name).unwrap(), key, serde_json::json!(v))?;
            }
            let started = Instant::now();
            let executed = runner.run(&p, level, target, scenario != "fresh", host);
            emit(scenario, level, i, started.elapsed().as_secs_f64() * 1000.0, executed);
        }
    }
    Ok(())
}
