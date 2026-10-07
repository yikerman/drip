//! Inspector help: the node's documentation, then its port
//! declared concrete port contracts.

use super::ports;
use drip::graph::{Graph, NodeId};
use egui::Ui;

pub fn show(ui: &mut Ui, graph: &Graph, id: NodeId) {
    let kind = graph.node(id).expect("selected node exists").kind;
    if !kind.documentation.is_empty() {
        ui.label(kind.documentation);
    }
    egui::Grid::new(("node help", id)).num_columns(2).show(ui, |ui| {
        for port in ports::texts(graph, id) {
            ui.weak(format!("{} {}", port.role.label(), port.port));
            ui.label(port.label);
            ui.end_row();
        }
        for check in kind.checks() {
            ui.weak("requires at evaluation");
            ui.label(check);
            ui.end_row();
        }
    });
    for &(name, url) in kind.references {
        crate::widgets::link(ui, name, url);
    }
    ui.add_space(6.0);
}
