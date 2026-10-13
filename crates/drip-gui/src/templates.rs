//! Built-in templates: a processing chain from [`drip::templates`] with the
//! views every template shows, laid out from the graph. New creates the
//! first. A user's own templates are `.drip-template` files that Open loads.
use crate::{
    model::{Graph, Kind, NodeId, Project},
    node_ui::{self, preview, scopes},
    registry::VIEWS,
    ui_state::LayoutField,
};
use drip::{graph::Dag, templates::Chain};
use egui::{Vec2, vec2};
use serde_json::Value as Json;
use std::collections::BTreeMap;

pub struct Template {
    pub name: &'static str,
    chain: fn(&mut Dag) -> drip::Result<Chain>,
}

/// Every built-in template, in menu order. The first is what New creates.
pub static TEMPLATES: &[Template] = &[
    Template { name: "Neutral restore", chain: drip::templates::neutral_restore },
    Template { name: "Neutral correction", chain: drip::templates::neutral_correction },
    Template { name: "All", chain: drip::templates::all },
];

impl Template {
    pub fn project(&self) -> drip::Result<Project> {
        let mut dag = Dag::new();
        let image = (self.chain)(&mut dag)?.output;
        let views = [
            preview::observe::add(&mut dag, Default::default())?.image,
            scopes::histogram::observe::add(&mut dag, Default::default())?.image,
            scopes::waveform::observe::add(&mut dag, Default::default())?.image,
            scopes::vectorscope::observe::add(&mut dag, ())?.image,
        ];
        for view in views {
            dag.connect(image, view)?;
        }
        let mut graph = Graph::from_dag(dag);
        lay_out(&mut graph)?;
        Ok(Project { graph, ui: Json::Null })
    }
}

/// The project New creates.
pub fn default_project() -> Project {
    TEMPLATES[0].project().expect("built-in templates load")
}

const GAP: f32 = 40.0;
/// How far each column of the chain sits below the previous one.
const STEP: f32 = 30.0;

/// Processing runs left to right, one column per step from the RAW file, and
/// descends a little per column so the metadata wires from the RAW node, which
/// sits below the chain, reach each node without crossing the others. Nodes
/// that show a view sit in a row above: the views first, then processing
/// nodes edited in their own view.
fn lay_out(graph: &mut Graph) -> drip::Result<()> {
    let only_views = |kind: &Kind| VIEWS.iter().any(|view| std::ptr::eq(*view, kind));
    let mut columns: BTreeMap<usize, Vec<NodeId>> = BTreeMap::new();
    let mut views = Vec::new();
    for (id, depth) in depths(graph) {
        match graph.kind(id).filter(|kind| node_ui::has_view(kind)) {
            Some(kind) => views.push((!only_views(kind), id)),
            None => columns.entry(depth).or_default().push(id),
        }
    }
    views.sort();
    let mut places = Vec::new();
    for (column, ids) in columns.into_values().enumerate() {
        let x = (node_ui::WIDTH + GAP) * column as f32;
        let y = if column == 0 { 800.0 } else { 520.0 + STEP * column as f32 };
        for (row, &id) in ids.iter().enumerate() {
            places.push((id, vec2(x, y + 160.0 * row as f32)));
        }
    }
    let mut x = 240.0;
    for (_, id) in views {
        places.push((id, vec2(x, 0.0)));
        x += graph.kind(id).and_then(view_size).map_or(0.0, |size| size.x) + GAP;
    }
    for (id, place) in places {
        let mut ui = Json::Null;
        LayoutField::Position.write(&mut ui, place);
        if let Some(size) = graph.kind(id).and_then(view_size) {
            LayoutField::ViewSize.write(&mut ui, size);
        }
        graph.set_ui(id, ui)?;
    }
    Ok(())
}

/// Each node's longest path from a node without inputs.
fn depths(graph: &Graph) -> BTreeMap<NodeId, usize> {
    let edges: Vec<_> = graph.edges().map(|(from, to)| (from.node(), to.node())).collect();
    let mut depths: BTreeMap<_, _> = graph.dag.nodes().map(|(id, _)| (id, 0)).collect();
    // Relaxing every edge until none deepens a node ends because the graph is acyclic.
    let mut changed = true;
    while changed {
        changed = false;
        for (from, to) in &edges {
            if depths[to] <= depths[from] {
                depths.insert(*to, depths[from] + 1);
                changed = true;
            }
        }
    }
    depths
}

/// Starting view sizes, larger than the editor's default so the views are
/// legible on first open.
fn view_size(kind: &Kind) -> Option<Vec2> {
    node_ui::has_view(kind).then(|| match kind.id {
        "preview" => vec2(672.0, 440.0),
        "perspective-transform" => vec2(480.0, 360.0),
        "vectorscope" => vec2(320.0, 320.0),
        _ => vec2(320.0, 240.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Built-ins skip the file a user's template loads from, so each must
    /// survive a save and reopen unchanged.
    #[test]
    fn built_in_templates_reopen_as_built() {
        for template in TEMPLATES {
            let text = template.project().unwrap().to_json().unwrap();
            assert_eq!(
                Project::from_json(&text).unwrap().to_json().unwrap(),
                text,
                "{}",
                template.name
            );
        }
    }

    #[test]
    fn views_and_exports_read_the_finished_image() {
        let readers =
            ["preview", "histogram", "waveform", "vectorscope", "export-tiff", "export-jpeg"];
        for template in TEMPLATES {
            let graph = template.project().unwrap().graph;
            let mut sources: Vec<_> = graph
                .nodes()
                .filter(|(_, node)| readers.contains(&node.kind.id))
                .map(|(id, _)| graph.source(graph.dag.input_port(id, "image").unwrap()))
                .collect();
            sources.dedup();
            assert_eq!(sources.len(), 1, "{}", template.name);
        }
    }

    #[test]
    fn raw_adjustments_are_shared_by_views_and_export_and_survive_reopening() {
        let mut project = default_project();
        let raw = project.graph.find("Bayer RAW").unwrap();
        let orientation = project.graph.find("Orientation").unwrap();
        let color = project.graph.find("Camera RGB to color RGB").unwrap();
        let adaptation = project.graph.find("Chromatic adaptation").unwrap();
        let gains = project.graph.find("Bayer site gains").unwrap();
        let highlights = project.graph.find("Opposed highlights").unwrap();
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
        let raw = reopened.graph.find("Bayer RAW").unwrap();
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
