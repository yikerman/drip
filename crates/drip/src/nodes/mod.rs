//! The built-in node kinds.

mod color;
mod export;
mod raw;
mod tone;
mod view;

pub use color::{BIN_2X2, CAMERA_TO_REC2020, WHITE_BALANCE};
pub use export::TIFF;
pub use raw::{READ, normalize};
pub use tone::SIGMOID;
pub use view::{HISTOGRAM, PREVIEW};

use serde_json::json;

use crate::graph::Port;
use crate::node::Registry;
use crate::project::Project;

pub fn registry() -> Registry {
    [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020, &SIGMOID, &PREVIEW, &HISTOGRAM, &TIFF]
        .into_iter()
        .fold(Registry::default(), Registry::with)
}

/// The prototype pipeline as a template with graph inputs `raw` (the raw file)
/// and `out` (the TIFF), laid out for the editor. The output profile is left
/// for the user to choose.
pub fn raw_to_tiff() -> Project {
    let reg = registry();
    let mut p = Project::default();
    let mut add = |kind, x: f64, y: f64| {
        let id = p.graph.add_node(kind);
        p.graph.set_ui(id, json!({ "pos": [x, y] })).expect("just added");
        id
    };
    let chain = [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020, &SIGMOID];
    let ids: Vec<_> =
        chain.iter().enumerate().map(|(i, kind)| add(kind, 195.0 * i as f64, 0.0)).collect();
    let views = [(&PREVIEW, -120.0), (&HISTOGRAM, 0.0), (&TIFF, 120.0)]
        .map(|(kind, y)| add(kind, 975.0, y));
    let mut connect = |from, output: &str, to, input: &str| {
        p.graph
            .connect(&reg, Port(from, output.into()), Port(to, input.into()))
            .expect("compatible built-in ports");
    };
    connect(ids[0], "mosaic", ids[1], "mosaic");
    connect(ids[1], "mosaic", ids[2], "mosaic");
    connect(ids[2], "image", ids[3], "image");
    connect(ids[3], "image", ids[4], "image");
    for id in views {
        connect(ids[4], "image", id, "image");
    }
    p.bind(&reg, ids[0], "path", "raw").expect("raw.read has a path");
    p.bind(&reg, views[2], "path", "out").expect("export.tiff has a path");
    p
}
