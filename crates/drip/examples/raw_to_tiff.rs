//! Runs the built-in raw-to-TIFF template on one raw file:
//!
//!     cargo run --example raw_to_tiff -- <raw> <profile> <out.tif> [template.drip]
//!
//! `<profile>` is a built-in profile (srgb, display_p3, rec2020) or an ICC
//! file. Giving a fourth path saves the template there for reuse.

use std::path::PathBuf;

use drip::eval::run_action;
use drip::{profile, templates};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [raw, profile, out, rest @ ..] = &args[..] else {
        return Err("usage: raw_to_tiff <raw> <profile> <out.tif> [template.drip]".into());
    };
    let mut p = templates::raw_to_tiff();
    let (read, export) =
        (p.graph.find("raw").expect("in the template"), p.graph.find("export").expect("too"));
    match profile.to_str().filter(|name| profile::BUILT_IN.contains(name)) {
        Some(name) => p.graph.set_param(export, "profile", json!(name))?,
        None => {
            p.graph.set_param(export, "profile", json!("file"))?;
            p.graph.set_param(export, "profile_file", json!(profile))?;
        }
    }
    if let Some(template) = rest.first() {
        std::fs::write(template, p.template().to_json())?;
    }
    p.graph.set_param(read, "path", json!(raw))?;
    p.graph.set_param(export, "path", json!(out))?;
    run_action(&p.graph, export, "export")?;
    Ok(())
}
