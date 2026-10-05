//! Inspector help. Port descriptions follow the kernel's declared contracts.

use super::ports;
use drip::node::NodeKind;
use egui::Ui;

pub fn show(ui: &mut Ui, kind: &NodeKind) {
    egui::Grid::new(("node help", kind.name)).num_columns(2).show(ui, |ui| {
        for input in kind.inputs() {
            ui.weak("input");
            ui.label(ports::label(input.requirement.name));
            ui.end_row();
        }
        for output in kind.outputs() {
            ui.weak("output");
            ui.label(ports::label(output.ty.name));
            ui.end_row();
        }
    });
    ui.add_space(6.0);
}
