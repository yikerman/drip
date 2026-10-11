//! The core starting pipeline, laid out with the editor's universal views.
use crate::model::{Graph, Port, Project, Registry};
use serde_json::json;

pub fn raw_to_tiff() -> Project {
    let pipeline = drip::node::templates::raw_pipeline(None).expect("valid built-in pipeline");
    let target = pipeline.target.expect("pipeline output");
    let mut graph = Graph::from_dag(pipeline.dag).expect("registered built-in nodes");
    let positions = [
        ("RAW", [0.0, 0.0]),
        ("Bayer site gains", [195.0, 0.0]),
        ("Reconstruct highlights (opposed)", [390.0, 0.0]),
        ("RCD demosaic", [585.0, 0.0]),
        ("Camera RGB to color RGB", [780.0, 0.0]),
        ("Chromatic adaptation", [975.0, 0.0]),
        ("Orientation", [1170.0, 0.0]),
        ("Exposure", [1365.0, 0.0]),
        ("Sigmoid", [1560.0, 0.0]),
    ];
    for (name, pos) in positions {
        graph.set_ui(graph.find(name).expect("pipeline node"), json!({"pos":pos})).unwrap();
    }
    let raw = graph.find("RAW").unwrap();
    for (kind, pos, size) in [
        ("view.preview", [1755.0, 0.0], json!([672, 440])),
        ("view.histogram", [2447.0, 0.0], json!([320, 240])),
        ("view.waveform", [2447.0, 300.0], json!([320, 240])),
        ("view.vectorscope", [2787.0, 0.0], json!([320, 320])),
        ("output.tiff", [1560.0, 220.0], json!(null)),
    ] {
        let id = graph.add_node(Registry.get(kind).expect("built-in observer")).unwrap();
        graph.set_ui(id, json!({"pos":pos, "size":size})).unwrap();
        graph.dag.connect_ids(target, graph.dag.input_port(id, "image").unwrap()).unwrap();
        if kind == "output.tiff" {
            graph.connect(Port(raw, "metadata".into()), Port(id, "metadata".into())).unwrap();
        }
    }
    Project { graph, ui: json!(null) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_adjustments_are_shared_by_views_and_export_and_survive_reopening() {
        let mut project = raw_to_tiff();
        let raw = project.graph.find("RAW").unwrap();
        let orientation = project.graph.find("Orientation").unwrap();
        let color = project.graph.find("Camera RGB to color RGB").unwrap();
        let adaptation = project.graph.find("Chromatic adaptation").unwrap();
        let gains = project.graph.find("Bayer site gains").unwrap();
        let highlights = project.graph.find("Reconstruct highlights (opposed)").unwrap();
        assert!(project.graph.find("Apply clipping-level gains").is_none());
        let exposure = project.graph.find("Exposure").unwrap();
        let sigmoid = project.graph.find("Sigmoid").unwrap();
        assert_eq!(project.graph.node(orientation).unwrap().params["orientation"], "from_metadata");
        for (from, output, to, input) in [
            (raw, "metadata", orientation, "metadata"),
            (raw, "metadata", gains, "metadata"),
            (raw, "metadata", color, "metadata"),
            (gains, "levels", highlights, "levels"),
            (color, "output", adaptation, "image"),
            (raw, "metadata", adaptation, "metadata"),
            (adaptation, "output", orientation, "image"),
            (orientation, "output", exposure, "image"),
        ] {
            assert_eq!(
                project.graph.source(&Port(to, input.into())),
                Some(Port(from, output.into()))
            );
        }
        for name in ["Preview", "Histogram", "Waveform", "Vectorscope", "Export TIFF"] {
            let id = project.graph.find(name).unwrap();
            assert_eq!(
                project.graph.source(&Port(id, "image".into())),
                Some(Port(sigmoid, "output".into()))
            );
        }
        project.graph.set_param(orientation, "orientation", json!("rotate_90_clockwise")).unwrap();
        project
            .graph
            .set_param(adaptation, "illuminant", json!({"custom":{"x":0.44757,"y":0.40745}}))
            .unwrap();
        let reopened = Project::from_json(&project.to_json().unwrap(), &Registry).unwrap();
        let orientation = reopened.graph.find("Orientation").unwrap();
        let raw = reopened.graph.find("RAW").unwrap();
        let adaptation = reopened.graph.find("Chromatic adaptation").unwrap();
        assert_eq!(
            reopened.graph.node(adaptation).unwrap().params["illuminant"],
            json!({"custom":{"x":0.44757,"y":0.40745}})
        );
        assert_eq!(
            reopened.graph.node(orientation).unwrap().params["orientation"],
            "rotate_90_clockwise"
        );
        assert_eq!(
            reopened.graph.source(&Port(orientation, "metadata".into())),
            Some(Port(raw, "metadata".into()))
        );
    }
}
