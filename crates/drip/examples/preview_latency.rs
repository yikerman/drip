//! Complete graph evaluation, including allocations and cache replacement, excluding drawing.
//! Run with RAYON_NUM_THREADS=1 or a worker count and a RAW path as the sole argument.

use std::{path::PathBuf, time::Instant};

use drip::{eval::Evaluator, templates};

struct Trace;
impl log::Log for Trace {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("drip::")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }
    fn flush(&self) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("DRIP_BENCH_TRACE").is_some() {
        log::set_logger(&Trace).unwrap();
        log::set_max_level(log::LevelFilter::Debug);
    }
    let path =
        std::env::args_os().nth(1).map(PathBuf::from).ok_or("usage: preview_latency <raw>")?;
    let mut project = templates::raw_to_tiff();
    let raw = project.graph.find("RAW").unwrap();
    project.graph.set_param(raw, "path", serde_json::json!(path))?;
    let targets =
        [project.graph.find("Preview").unwrap(), project.graph.find("Histogram").unwrap()];
    let mut evaluator = Evaluator::default();
    let mut measure = |level| -> Result<f64, Box<dyn std::error::Error>> {
        let start = Instant::now();
        evaluator.evaluate(&project.graph, level, &targets);
        for target in targets {
            evaluator.result(target).unwrap().as_ref().map_err(Clone::clone)?;
        }
        Ok(start.elapsed().as_secs_f64() * 1000.0)
    };
    println!("initial level 3 (RAW load + thread-pool startup): {:.1} ms", measure(3)?);
    // Warm every shape, then alternate levels so the evaluator recomputes nodes.
    for level in [2, 1, 0] {
        measure(level)?;
    }
    let mut samples: [Vec<f64>; 4] = Default::default();
    for _ in 0..7 {
        for level in [3, 2, 1, 0] {
            samples[level as usize].push(measure(level)?);
        }
    }
    for (level, samples) in samples.iter_mut().enumerate().rev() {
        samples.sort_by(f64::total_cmp);
        println!(
            "level {level}: median {:.1} ms, range {:.1}–{:.1} ms",
            samples[3], samples[0], samples[6]
        );
    }
    Ok(())
}
