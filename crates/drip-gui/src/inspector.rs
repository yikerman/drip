//! The sidebar: the selected node's help and controls, and the template's inputs.

use crate::model::Graph;
use egui::Ui;

use crate::editing::{Edit, Frame, NodeCx};
use crate::node_ui::{help, parameters};

pub fn selected_node(ui: &mut Ui, cx: &mut NodeCx) {
    parameters::heading(ui, cx);
    help::show(ui, cx.graph(), cx.id());
    parameters::controls(ui, cx);
}

/// The template's inputs, one per external parameter of each node, edited
/// in place.
pub fn inputs(ui: &mut Ui, graph: &mut Graph, frame: &mut Frame) {
    let inputs: Vec<_> = graph.inputs().collect();
    if inputs.is_empty() {
        return;
    }
    ui.weak("Template parameters");
    egui::Grid::new("inputs").num_columns(2).show(ui, |ui| {
        for (id, param) in inputs {
            let node = graph.node(id).expect("listed");
            let spec = node.kind.param(param).expect("listed");
            let label = ui.label(format!("{} · {}", node.name, spec.label));
            if !spec.documentation.is_empty() {
                label.on_hover_text(spec.documentation);
            }
            if let Some(value) =
                parameters::edit_parameter(ui, egui::Id::new(("input", id, param)), &node, spec)
            {
                frame.edit(graph, Edit::Param(id, param, value));
            }
            ui.end_row();
        }
    });
}
