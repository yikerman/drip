//! Built-in templates.

use serde_json::json;

use crate::graph::{Graph, NodeId, Port};
use crate::node::NodeKind;
use crate::nodes::*;
use crate::project::Project;

/// The prototype pipeline, laid out for the editor: the processing chain on
/// the left, a 3:2 preview beside its end and the histogram beyond that, the
/// exporter under the chain. Its inputs are the raw reader's path and the
/// exporter's path; the output profile defaults to sRGB.
pub fn raw_to_tiff() -> Project {
    let mut g = Graph::default();
    let mut add = |kind: &'static NodeKind, ui| {
        let id = g.add_node(kind);
        g.set_ui(id, ui).expect("just added");
        id
    };
    let chain = [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020, &EXPOSURE, &SIGMOID];
    let chain: Vec<_> = chain
        .iter()
        .enumerate()
        .map(|(i, kind)| add(kind, json!({ "pos": [195.0 * i as f64, 0.0] })))
        .collect();
    let sinks = [
        add(&PREVIEW, json!({ "pos": [1170.0, 0.0], "size": [672.0, 440.0] })),
        add(&HISTOGRAM, json!({ "pos": [1862.0, 0.0], "size": [320.0, 240.0] })),
        add(&TIFF, json!({ "pos": [975.0, 80.0] })),
    ];
    let mut connect = |from: NodeId, output: &str, to: NodeId, input: &str| {
        g.connect(Port(from, output.into()), Port(to, input.into()))
            .expect("compatible built-in ports");
    };
    connect(chain[0], "mosaic", chain[1], "mosaic");
    connect(chain[1], "mosaic", chain[2], "mosaic");
    connect(chain[2], "image", chain[3], "image");
    connect(chain[3], "image", chain[4], "image");
    connect(chain[4], "image", chain[5], "image");
    for id in sinks {
        connect(chain[5], "image", id, "image");
    }
    Project { graph: g, ui: serde_json::Value::Null }
}
