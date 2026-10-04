//! Runs the prototype pipeline on one raw file:
//!
//!     cargo run --example raw_to_tiff -- <raw> <output.icc> <out.tif> [template.json]
//!
//! The graph is a template with inputs `raw` and `out`; giving a fourth path
//! saves it there for reuse.

use std::path::PathBuf;

use drip::eval::run_action;
use drip::graph::Port;
use drip::nodes;
use drip::project::Project;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [raw, profile, out, rest @ ..] = &args[..] else {
        return Err("usage: raw_to_tiff <raw> <output.icc> <out.tif> [template.json]".into());
    };
    let reg = nodes::registry();
    let mut p = Project::default();
    let kinds = [
        &nodes::READ,
        &nodes::WHITE_BALANCE,
        &nodes::BIN_2X2,
        &nodes::CAMERA_TO_REC2020,
        &nodes::SIGMOID,
    ];
    let ids: Vec<_> = kinds.iter().map(|kind| p.graph.add_node(kind)).collect();
    for (pair, kind) in ids.windows(2).zip(&kinds[1..]) {
        let output = reg.get(&p.graph.node(pair[0]).unwrap().kind).unwrap().outputs[0].name;
        p.graph.connect(
            &reg,
            Port(pair[0], output.into()),
            Port(pair[1], kind.inputs[0].name.into()),
        )?;
    }
    let (preview, histogram, export) = (
        p.graph.add_node(&nodes::PREVIEW),
        p.graph.add_node(&nodes::HISTOGRAM),
        p.graph.add_node(&nodes::TIFF),
    );
    for id in [preview, histogram, export] {
        p.graph.connect(&reg, Port(ids[4], "image".into()), Port(id, "image".into()))?;
    }
    p.bind(&reg, ids[0], "path", "raw")?;
    p.bind(&reg, export, "path", "out")?;
    p.graph.set_param(&reg, export, "profile", json!(profile))?;
    if let Some(template) = rest.first() {
        std::fs::write(template, p.to_json())?;
    }
    p.set_argument(&reg, "raw", json!(raw))?;
    p.set_argument(&reg, "out", json!(out))?;
    run_action(&p, &reg, export, "export")?;
    Ok(())
}
