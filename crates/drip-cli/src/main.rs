//! Batch evaluation and TIFF export using the same checked graph as the GUI.
use drip::{eval::Evaluator, node::data::Rgb, ports::Cpu, runtime::RuntimeContext};
use std::{path::PathBuf, process::ExitCode};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: drip-cli INPUT.raw|PROJECT.drip OUTPUT.tiff".into());
    }
    let runtime = RuntimeContext::from_env()?;
    let input = PathBuf::from(&args[0]);
    let project = if input.extension().is_some_and(|s| s == "drip") {
        drip::project::Project::target_from_json(&std::fs::read_to_string(&input)?)?
    } else {
        drip::node::templates::raw_to_rgb(&input)?
    };
    let target =
        project.dag.typed_output::<Rgb>(project.target.ok_or("project has no export target")?)?;
    let image =
        Evaluator::new(runtime).evaluate::<Cpu<Rgb>>(&project.dag, &Default::default(), target)?;
    let export =
        drip::node::export::Export { path: Some(PathBuf::from(&args[1])), ..Default::default() };
    drip::node::export::write(&export, &image.desc, &image.data, None, &Default::default())?;
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("drip-cli: {e}");
            ExitCode::FAILURE
        }
    }
}
