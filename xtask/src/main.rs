//! Assemble CI artifacts from Cargo's reported executable paths.

use std::{
    env,
    error::Error,
    fs,
    io::BufReader,
    path::Path,
    process::{Command, Stdio},
};

use cargo_metadata::Message;
use clap::{Parser, Subcommand};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

mod target;

#[derive(Parser)]
#[command(about = "Drip development tasks")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Build both frontends and replace ./dist/ with their executables.
    Dist {
        /// Rust target triple; omitted to build for the compiler's host.
        #[arg(long)]
        target: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().task {
        Task::Dist { target } => dist(target.as_deref()),
    }
}

fn dist(target: Option<&str>) -> Result<()> {
    let (target, feature) = target::resolve(target)?;
    eprintln!("building {target} with {feature}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut cargo = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cargo.current_dir(root).args([
        "build",
        "--locked",
        "--profile",
        "dist",
        "--bins",
        "--package",
        "drip-gui",
        "--package",
        "drip-cli",
        "--message-format=json-render-diagnostics",
        "--target",
        &target,
        "--features",
        feature,
    ]);
    let mut build = cargo.stdout(Stdio::piped()).spawn()?;
    let mut executables = Vec::new();
    // Cargo owns target directories and platform suffixes, including user overrides.
    for message in Message::parse_stream(BufReader::new(build.stdout.take().unwrap())) {
        match message? {
            Message::CompilerArtifact(artifact) if artifact.target.is_bin() => {
                executables.extend(artifact.executable);
            }
            Message::TextLine(line) => println!("{line}"),
            _ => {}
        }
    }
    if !build.wait()?.success() {
        return Err("distribution build failed".into());
    }

    let output = root.join("dist");
    if output.exists() {
        fs::remove_dir_all(&output)?;
    }
    fs::create_dir(&output)?;
    for executable in executables {
        let destination = output.join(executable.file_name().unwrap());
        fs::copy(&executable, &destination)?;
        eprintln!("staged {}", destination.display());
    }
    Ok(())
}
