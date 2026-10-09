//! The core starting pipeline, laid out with the editor's universal views.
use crate::model::{Graph, Port, Project, Registry};
use serde_json::json;

pub fn raw_to_tiff() -> Project {
    let pipeline = drip::node::templates::raw_pipeline(None).expect("valid built-in pipeline");
    let target = pipeline.target.expect("pipeline output");
    let mut graph = Graph::from_dag(pipeline.dag).expect("registered built-in nodes");
    let positions = [
        ("RAW", [0.0, 0.0]),
        ("White balance", [195.0, 0.0]),
        ("Balance clipping levels", [195.0, 200.0]),
        ("Highlights", [390.0, 0.0]),
        ("Demosaic", [585.0, 0.0]),
        ("Camera to RGB", [780.0, 0.0]),
        ("Exposure", [975.0, 0.0]),
        ("Sigmoid", [1170.0, 0.0]),
    ];
    for (name, pos) in positions {
        graph.set_ui(graph.find(name).expect("pipeline node"), json!({"pos":pos})).unwrap();
    }
    let raw = graph.find("RAW").unwrap();
    for (kind, pos, size) in [
        ("view.preview", [1365.0, 0.0], json!([672, 440])),
        ("view.histogram", [2057.0, 0.0], json!([320, 240])),
        ("view.waveform", [2057.0, 300.0], json!([320, 240])),
        ("view.vectorscope", [2397.0, 0.0], json!([320, 320])),
        ("export.tiff", [1170.0, 220.0], json!(null)),
    ] {
        let id = graph.add_node(Registry.get(kind).expect("built-in observer")).unwrap();
        graph.set_ui(id, json!({"pos":pos, "size":size})).unwrap();
        graph.dag.connect_ids(target, graph.dag.input_port(id, "image").unwrap()).unwrap();
        if kind == "export.tiff" {
            graph.connect(Port(raw, "metadata".into()), Port(id, "metadata".into())).unwrap();
        }
    }
    Project { graph, ui: json!(null) }
}
