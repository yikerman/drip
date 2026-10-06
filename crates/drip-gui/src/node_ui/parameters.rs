//! Node parameter panels and schema widgets, shared by the inspector and pop-outs.

use drip::param::ParamKind;
use egui::{Sense, Ui};
use serde_json::{Value as Json, json};

use crate::editing::NodeCx;

/// A node's label, kind, parameters and actions, without inspector help.
pub fn panel(ui: &mut Ui, cx: &mut NodeCx) {
    heading(ui, cx);
    controls(ui, cx);
}

pub fn heading(ui: &mut Ui, cx: &mut NodeCx) {
    let (id, node) = (cx.id(), cx.node().clone());
    if let Some(label) = edit_text(ui, egui::Id::new(("label", id)), &node.label) {
        cx.set_label(&label);
    }
    ui.weak(node.kind.name);
}

pub fn controls(ui: &mut Ui, cx: &mut NodeCx) {
    let kind = cx.node().kind;
    crate::node_ui::of(kind).controls(ui, cx);
    for action in kind.actions() {
        if ui.add_enabled(!cx.action_running(), egui::Button::new(action.name)).clicked() {
            cx.run(action.name);
        }
    }
}

/// Default schema controls, shared by custom node panels and generic nodes.
pub fn schema(ui: &mut Ui, cx: &mut NodeCx) {
    let (id, node) = (cx.id(), cx.node().clone());
    egui::Grid::new(("params", id)).num_columns(2).show(ui, |ui| {
        for spec in node.kind.params {
            let external = node.external.contains(spec.name);
            let text = if external { format!("{} (input)", spec.name) } else { spec.name.into() };
            let name =
                ui.label(text).interact(Sense::click()).on_hover_text("right-click to change");
            if let Some(value) =
                edit_value(ui, egui::Id::new((id, spec.name)), &spec.kind, &node.params[spec.name])
            {
                cx.set_param(spec.name, value);
            }
            name.context_menu(|ui| {
                let toggle = if external { "Fix in template" } else { "Make template input" };
                if ui.button(toggle).clicked() {
                    cx.set_external(spec.name, !external);
                    ui.close();
                }
            });
            ui.end_row();
        }
    });
}

/// A widget for one parameter value; returns the new value when edited.
pub fn edit_value(ui: &mut Ui, id: egui::Id, kind: &ParamKind, value: &Json) -> Option<Json> {
    match *kind {
        ParamKind::Float { min, max, .. } => {
            let mut v = value.as_f64().expect("validated float");
            ui.add(egui::Slider::new(&mut v, min..=max)).changed().then(|| json!(v))
        }
        ParamKind::Int { min, max, .. } => {
            let mut v = value.as_i64().expect("validated int");
            ui.add(egui::Slider::new(&mut v, min..=max)).changed().then(|| json!(v))
        }
        ParamKind::Bool { .. } => {
            let mut v = value.as_bool().expect("validated bool");
            ui.checkbox(&mut v, "").changed().then(|| json!(v))
        }
        ParamKind::Choice { options, .. } => {
            let current = value.as_str().expect("validated choice");
            let mut chosen = None;
            egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
                for option in options {
                    if ui.selectable_label(*option == current, *option).clicked() {
                        chosen = Some(json!(option));
                    }
                }
            });
            chosen
        }
        ParamKind::Path { output } => {
            let path = value.as_str().map(std::path::Path::new);
            let name =
                path.and_then(|p| p.file_name()).map_or("none".into(), |n| n.to_string_lossy());
            let mut chosen = None;
            ui.horizontal(|ui| {
                if ui.button("…").clicked() {
                    let dialog = rfd::FileDialog::new();
                    let dialog = match path.and_then(|p| p.parent()) {
                        Some(dir) => dialog.set_directory(dir),
                        None => dialog,
                    };
                    chosen = if output { dialog.save_file() } else { dialog.pick_file() }
                        .map(|p| json!(p));
                }
                let label = ui.label(name);
                if let Some(path) = path {
                    label.on_hover_text(path.display().to_string());
                }
            });
            chosen
        }
    }
}

/// A single-line text field whose edit is returned once the field stops having
/// focus, however that happens; while editing, the draft lives in egui's memory.
fn edit_text(ui: &mut Ui, id: egui::Id, current: &str) -> Option<String> {
    let editing = ui.data_mut(|d| d.get_temp::<String>(id));
    let mut draft = editing.clone().unwrap_or_else(|| current.to_owned());
    let response = ui.add(egui::TextEdit::singleline(&mut draft).id(id).frame(egui::Frame::NONE));
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, draft));
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(id));
    (editing.is_some() && draft != current).then_some(draft)
}
