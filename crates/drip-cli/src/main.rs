//! Batch evaluation and TIFF export using the same checked graph as the GUI.
use drip::{eval::Evaluator, node::data::ColorRgb, ports::Cpu, runtime::RuntimeContext};
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
    let target = project
        .dag
        .typed_output::<ColorRgb>(project.target.ok_or("project has no export target")?)?;
    let image = Evaluator::new(runtime).evaluate::<Cpu<ColorRgb>>(
        &project.dag,
        &Default::default(),
        target,
    )?;
    let profile = drip::node::profile::Output::load(&Default::default(), &Default::default())?;
    let bytes = drip::node::export::tiff(&image.desc, &image.data, &profile, false, None)?;
    std::fs::write(PathBuf::from(&args[1]), bytes)?;
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
