//! The core starting pipeline, laid out with the editor's universal views.
use super::{preview, scopes};
use crate::model::{Graph, Project};
use serde_json::json;

pub fn raw_to_tiff() -> Project {
    let (mut dag, pipeline) =
        drip::node::templates::raw_to_tiff().expect("valid built-in pipeline");
    let views = [
        preview::observe::add(&mut dag, Default::default()).map(|v| v.image),
        scopes::histogram::observe::add(&mut dag, Default::default()).map(|v| v.image),
        scopes::waveform::observe::add(&mut dag, Default::default()).map(|v| v.image),
        scopes::vectorscope::observe::add(&mut dag, ()).map(|v| v.image),
    ];
    let views = views.map(|view| view.expect("registered view"));
    for view in views {
        dag.connect(pipeline.sigmoid.output, view).expect("views accept rendered RGB");
    }
    let mut graph = Graph::from_dag(dag).expect("registered built-in nodes");
    // Align views on the top edge, stack histogram/waveform in one column,
    // and stagger processing through Orientation to separate metadata branches.
    // Exposure/Sigmoid align with Orientation; RAW and TIFF sit below the chain.
    let layout = [
        (pipeline.raw.node, json!({"pos": [0.0, 800.0]})),
        (pipeline.gains.node, json!({"pos": [195.0, 550.0]})),
        (pipeline.highlights.node, json!({"pos": [390.0, 565.0]})),
        (pipeline.demosaic.node, json!({"pos": [585.0, 600.0]})),
        (pipeline.color.node, json!({"pos": [780.0, 635.0]})),
        (pipeline.adaptation.node, json!({"pos": [975.0, 670.0]})),
        (pipeline.orientation.node, json!({"pos": [1170.0, 720.0]})),
        (pipeline.exposure.node, json!({"pos": [1365.0, 720.0]})),
        (pipeline.sigmoid.node, json!({"pos": [1560.0, 720.0]})),
        (pipeline.export.node, json!({"pos": [1560.0, 900.0], "size": null})),
        (views[0].node(), json!({"pos": [195.0, 0.0], "size": [672, 440]})),
        (views[1].node(), json!({"pos": [975.0, 0.0], "size": [320, 240]})),
        (views[2].node(), json!({"pos": [975.0, 320.0], "size": [320, 240]})),
        (views[3].node(), json!({"pos": [1365.0, 0.0], "size": [320, 320]})),
    ];
    for (id, ui) in layout {
        graph.set_ui(id, ui).unwrap();
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
                project.graph.source(project.graph.dag.input_port(to, input).unwrap()),
                Some(project.graph.dag.output_port(from, output).unwrap())
            );
        }
        for name in ["Preview", "Histogram", "Waveform", "Vectorscope", "Export TIFF"] {
            let id = project.graph.find(name).unwrap();
            assert_eq!(
                project.graph.source(project.graph.dag.input_port(id, "image").unwrap()),
                Some(project.graph.dag.output_port(sigmoid, "output").unwrap())
            );
        }
        project.graph.set_param(orientation, "orientation", json!("rotate_90_clockwise")).unwrap();
        project
            .graph
            .set_param(adaptation, "illuminant", json!({"custom":{"x":0.44757,"y":0.40745}}))
            .unwrap();
        let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
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
            reopened.graph.source(reopened.graph.dag.input_port(orientation, "metadata").unwrap()),
            Some(reopened.graph.dag.output_port(raw, "metadata").unwrap())
        );
    }
}
