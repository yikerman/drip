//! The sidebar: the selected node's help and controls, and the template's inputs.

use drip::graph::Graph;
use egui::Ui;

use crate::editing::{Edit, Frame, NodeCx};
use crate::node_ui::{help, parameters};

pub fn selected_node(ui: &mut Ui, cx: &mut NodeCx) {
    parameters::heading(ui, cx);
    help::show(ui, cx.node().kind);
    parameters::controls(ui, cx);
}

/// The template's inputs, one per external parameter of each node, edited
/// in place.
pub fn inputs(ui: &mut Ui, graph: &mut Graph, frame: &mut Frame) {
    let inputs: Vec<_> = graph.inputs().collect();
    if inputs.is_empty() {
        return;
    }
    ui.weak("inputs");
    egui::Grid::new("inputs").num_columns(2).show(ui, |ui| {
        for (id, param) in inputs {
            let node = graph.node(id).expect("listed");
            ui.label(format!("{} · {param}", node.label));
            let kind = node.kind.param(param).expect("listed").kind;
            let value = node.params[param].clone();
            if let Some(value) =
                parameters::edit_value(ui, egui::Id::new(("input", id, param)), &kind, &value)
            {
                frame.edit(graph, Edit::Param(id, param, value));
            }
            ui.end_row();
        }
    });
}
