//! Runs the built-in raw-to-TIFF template on one raw file:
//!
//!     cargo run --example raw_to_tiff -- <raw> <profile> <out.tif> [template.json]
//!
//! `<profile>` is a built-in profile (srgb, display_p3, rec2020) or an ICC
//! file. Giving a fourth path saves the template there for reuse.

use std::path::PathBuf;

use drip::eval::run_action;
use drip::{nodes, profile};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [raw, profile, out, rest @ ..] = &args[..] else {
        return Err("usage: raw_to_tiff <raw> <profile> <out.tif> [template.json]".into());
    };
    let reg = nodes::registry();
    let mut p = nodes::raw_to_tiff();
    let export = p.graph.find("export.tiff").expect("in the template");
    match profile.to_str().filter(|name| profile::BUILT_IN.contains(name)) {
        Some(name) => p.graph.set_param(&reg, export, "profile", json!(name))?,
        None => {
            p.graph.set_param(&reg, export, "profile", json!("file"))?;
            p.graph.set_param(&reg, export, "profile_file", json!(profile))?;
        }
    }
    if let Some(template) = rest.first() {
        std::fs::write(template, p.to_json())?;
    }
    p.set_argument(&reg, "raw", json!(raw))?;
    p.set_argument(&reg, "out", json!(out))?;
    run_action(&p, &reg, export, "export")?;
    Ok(())
}
