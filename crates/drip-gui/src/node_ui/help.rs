//! Inspector help. Port descriptions follow the kernel's declared contracts.

use super::{documentation, ports};
use drip::node::NodeKind;
use egui::Ui;

pub fn show(ui: &mut Ui, kind: &NodeKind) {
    let doc = documentation::of(kind);
    if let Some(doc) = doc {
        ui.label(doc.description);
    }
    egui::Grid::new(("node help", kind.id)).num_columns(2).show(ui, |ui| {
        for input in kind.inputs() {
            ui.weak(if input.requirement.optional { "optional input" } else { "input" });
            ui.label(ports::label(input.requirement.name));
            ui.end_row();
        }
        for output in kind.outputs() {
            ui.weak("output");
            ui.label(ports::label(output.ty.name));
            ui.end_row();
        }
        if let Some((kind, name)) = doc.and_then(|doc| doc.result) {
            ui.weak(kind);
            ui.label(name);
            ui.end_row();
        }
    });
    if let Some((name, url)) = doc.and_then(|doc| doc.reference) {
        crate::widgets::link(ui, name, url);
    }
    ui.add_space(6.0);
}
