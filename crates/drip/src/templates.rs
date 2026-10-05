//! Built-in templates.

use serde_json::json;

use crate::graph::{Graph, NodeId, Port};
use crate::node::NodeKind;
use crate::nodes::*;
use crate::project::Project;

/// The prototype pipeline, laid out for the editor. Its inputs are the raw
/// reader's path and the exporter's path; the output profile defaults to sRGB.
pub fn raw_to_tiff() -> Project {
    let mut g = Graph::default();
    let mut add = |kind: &'static NodeKind, x: f64, y: f64| {
        let id = g.add_node(kind);
        g.set_ui(id, json!({ "pos": [x, y] })).expect("just added");
        id
    };
    let chain = [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020, &SIGMOID];
    let chain: Vec<_> =
        chain.iter().enumerate().map(|(i, kind)| add(kind, 195.0 * i as f64, 0.0)).collect();
    let sinks = [(&PREVIEW, -260.0), (&HISTOGRAM, 0.0), (&TIFF, 160.0)]
        .map(|(kind, y)| add(kind, 975.0, y));
    let mut connect = |from: NodeId, output: &str, to: NodeId, input: &str| {
        g.connect(Port(from, output.into()), Port(to, input.into()))
            .expect("compatible built-in ports");
    };
    connect(chain[0], "mosaic", chain[1], "mosaic");
    connect(chain[1], "mosaic", chain[2], "mosaic");
    connect(chain[2], "image", chain[3], "image");
    connect(chain[3], "image", chain[4], "image");
    for id in sinks {
        connect(chain[4], "image", id, "image");
    }
    Project { graph: g, ui: serde_json::Value::Null }
}
