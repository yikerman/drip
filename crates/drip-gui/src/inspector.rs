//! The sidebar: the selected node's parameters and the template's inputs.
//! Widgets come from parameter schemas; every edit goes through the graph's
//! validated operations.

use drip::graph::{Graph, NodeId};
use drip::param::ParamKind;
use egui::{Sense, Ui};
use serde_json::{Value as Json, json};

/// What one frame of the inspector reads from the app and reports back.
#[derive(Default)]
pub struct Frame {
    /// Disables actions while one runs.
    pub action_running: bool,
    /// An edit the graph refused.
    pub refused: Option<String>,
    pub changed: bool,
    /// An action the user asked to run.
    pub action: Option<(NodeId, &'static str)>,
}

impl Frame {
    fn set_param(&mut self, graph: &mut Graph, id: NodeId, name: &str, value: Json) {
        match graph.set_param(id, name, value) {
            Ok(()) => self.changed = true,
            Err(e) => self.refused = Some(e.to_string()),
        }
    }
}

pub fn node(ui: &mut Ui, graph: &mut Graph, id: NodeId, frame: &mut Frame) {
    let node = graph.node(id).expect("selected nodes exist").clone();
    if let Some(label) = edit_text(ui, egui::Id::new(("label", id)), &node.label)
        && let Err(e) = graph.set_label(id, &label)
    {
        frame.refused = Some(e.to_string());
    }
    ui.weak(node.kind.name);
    egui::Grid::new(("params", id)).num_columns(2).show(ui, |ui| {
        for spec in node.kind.params {
            let external = node.external.contains(spec.name);
            let text = if external { format!("{} (input)", spec.name) } else { spec.name.into() };
            let name =
                ui.label(text).interact(Sense::click()).on_hover_text("right-click to change");
            if let Some(value) =
                edit_value(ui, egui::Id::new((id, spec.name)), &spec.kind, &node.params[spec.name])
            {
                frame.set_param(graph, id, spec.name, value);
            }
            name.context_menu(|ui| {
                let toggle = if external { "Fix in template" } else { "Make template input" };
                if ui.button(toggle).clicked() {
                    graph.set_external(id, spec.name, !external).expect("the kind's parameter");
                    ui.close();
                }
            });
            ui.end_row();
        }
    });
    for action in node.kind.actions {
        if ui.add_enabled(!frame.action_running, egui::Button::new(action.name)).clicked() {
            frame.action = Some((id, action.name));
        }
    }
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
            if let Some(value) = edit_value(ui, egui::Id::new(("input", id, param)), &kind, &value)
            {
                frame.set_param(graph, id, param, value);
            }
            ui.end_row();
        }
    });
}

/// A widget for one parameter value; returns the new value when edited.
fn edit_value(ui: &mut Ui, id: egui::Id, kind: &ParamKind, value: &Json) -> Option<Json> {
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
