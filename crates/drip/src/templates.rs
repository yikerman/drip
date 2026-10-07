//! Built-in templates.

use serde_json::json;

use crate::graph::{Graph, Port};
use crate::node::NodeKind;
use crate::nodes::*;
use crate::project::Project;

/// The prototype pipeline, laid out for the editor: the processing chain on
/// the left, a 3:2 preview beside its end and three scopes beyond that, the
/// exporter under the chain, which also receives the RAW's camera metadata.
/// Its inputs are the raw reader's path and the exporter's path; the output
/// profile defaults to sRGB.
pub fn raw_to_tiff() -> Project {
    let mut g = Graph::default();
    let mut add = |kind: &'static NodeKind, ui| {
        let id = g.add_node(kind);
        g.set_ui(id, ui).expect("just added");
        id
    };
    let kinds: [&NodeKind; 7] =
        [&READ, &WHITE_BALANCE, &HIGHLIGHTS, &RCD, &CAMERA_TO_REC2020, &EXPOSURE, &SIGMOID];
    let chain: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| add(kind, json!({"pos":[195.0*i as f64,0.0]})))
        .collect();
    let end = 195.0 * (chain.len() - 1) as f64;
    let sinks = [
        add(&PREVIEW, json!({"pos":[end+195.0,0.0],"size":[672.0,440.0]})),
        add(&HISTOGRAM, json!({"pos":[end+887.0,0.0],"size":[320.0,240.0]})),
        add(&WAVEFORM, json!({"pos":[end+887.0,300.0],"size":[320.0,240.0]})),
        add(&VECTORSCOPE, json!({"pos":[end+1227.0,0.0],"size":[320.0,320.0]})),
        add(&TIFF, json!({"pos":[end,80.0]})),
    ];
    for i in 1..chain.len() {
        g.connect(
            Port(chain[i - 1], kinds[i - 1].outputs().next().unwrap().name.into()),
            Port(chain[i], kinds[i].inputs().next().unwrap().name.into()),
        )
        .expect("compatible built-in ports");
    }
    for id in sinks {
        g.connect(Port(*chain.last().unwrap(), "image".into()), Port(id, "image".into()))
            .expect("compatible built-in ports");
    }
    let export = *sinks.last().unwrap();
    g.connect(Port(chain[0], "metadata".into()), Port(export, "metadata".into()))
        .expect("compatible built-in ports");
    Project { graph: g, ui: serde_json::Value::Null }
}
