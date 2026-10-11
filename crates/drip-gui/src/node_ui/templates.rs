//! The core starting pipeline, laid out with the editor's universal views.
use super::{preview, scopes};
use crate::model::{Graph, Project};
use serde_json::json;

pub fn raw_to_tiff() -> Project {
    let (mut dag, rendered) =
        drip::node::templates::raw_to_tiff().expect("valid built-in pipeline");
    let views = [
        preview::observe::add(&mut dag, Default::default()).map(|v| v.image),
        scopes::histogram::observe::add(&mut dag, Default::default()).map(|v| v.image),
        scopes::waveform::observe::add(&mut dag, Default::default()).map(|v| v.image),
        scopes::vectorscope::observe::add(&mut dag, ()).map(|v| v.image),
    ];
    for view in views {
        dag.connect(rendered, view.expect("registered view")).expect("views accept rendered RGB");
    }
    let mut graph = Graph::from_dag(dag).expect("registered built-in nodes");
    // Align views on the top edge, stack histogram/waveform in one column,
    // and stagger processing through Orientation to separate metadata branches.
    // Exposure/Sigmoid align with Orientation; RAW and TIFF sit below the chain.
    let layout = [
        ("RAW", json!({"pos": [0.0, 800.0]})),
        ("Bayer site gains", json!({"pos": [195.0, 550.0]})),
        ("Reconstruct highlights (opposed)", json!({"pos": [390.0, 565.0]})),
        ("RCD demosaic", json!({"pos": [585.0, 600.0]})),
        ("Camera RGB to color RGB", json!({"pos": [780.0, 635.0]})),
        ("Chromatic adaptation", json!({"pos": [975.0, 670.0]})),
        ("Orientation", json!({"pos": [1170.0, 720.0]})),
        ("Exposure", json!({"pos": [1365.0, 720.0]})),
        ("Sigmoid", json!({"pos": [1560.0, 720.0]})),
        ("Export TIFF", json!({"pos": [1560.0, 900.0], "size": null})),
        ("Preview", json!({"pos": [195.0, 0.0], "size": [672, 440]})),
        ("Histogram", json!({"pos": [975.0, 0.0], "size": [320, 240]})),
        ("Waveform", json!({"pos": [975.0, 320.0], "size": [320, 240]})),
        ("Vectorscope", json!({"pos": [1365.0, 0.0], "size": [320, 320]})),
    ];
    for (name, ui) in layout {
        graph.set_ui(graph.find(name).expect("template node"), ui).unwrap();
    }
    Project { graph, ui: json!(null) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Port, Registry};

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
