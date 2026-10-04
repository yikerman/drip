//! Panels for the selected node's parameters, the graph's inputs and the
//! histogram. Widgets come from parameter schemas; every edit goes through the
//! project's validated operations.

use drip::graph::NodeId;
use drip::param::ParamKind;
use drip::value::Histogram;
use egui::{Align2, FontId, Pos2, Sense, Stroke, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::app::App;
use crate::theme;

pub fn node(app: &mut App, ui: &mut Ui, id: NodeId) {
    let node = app.project.graph.node(id).expect("selected nodes exist").clone();
    if let Some(label) = edit_text(ui, egui::Id::new(("label", id)), &node.label)
        && let Err(e) = app.project.graph.set_label(id, &label)
    {
        app.report(Err(e.to_string()));
    }
    ui.weak(&node.kind);
    let Some(kind) = app.registry.get(&node.kind).filter(|k| k.version == node.kind_version) else {
        ui.label("This node's kind is unknown to this version of Drip; it is kept as is.");
        return;
    };
    egui::Grid::new(("params", id)).num_columns(2).show(ui, |ui| {
        for spec in kind.params {
            let name = ui.label(spec.name).interact(Sense::click());
            let bound = node.bindings.get(spec.name);
            match bound {
                Some(input) => {
                    ui.weak(format!("input {input}"));
                }
                None => {
                    if let Some(value) = edit_value(
                        ui,
                        egui::Id::new((id, spec.name)),
                        &spec.kind,
                        &node.params[spec.name],
                    ) && let Err(e) =
                        app.project.graph.set_param(&app.registry, id, spec.name, value)
                    {
                        app.report(Err(e.to_string()));
                    }
                }
            }
            name.context_menu(|ui| {
                if bound.is_some() {
                    if ui.button("Unbind").clicked() {
                        app.project.unbind(id, spec.name);
                        ui.close();
                    }
                    return;
                }
                let mut inputs: Vec<String> =
                    app.project.graph.inputs().into_iter().map(String::from).collect();
                if !inputs.iter().any(|i| i == spec.name) {
                    inputs.push(spec.name.into());
                }
                for input in inputs {
                    if ui.button(format!("Bind to input {input}")).clicked() {
                        if let Err(e) = app.project.bind(&app.registry, id, spec.name, &input) {
                            app.report(Err(e.to_string()));
                        }
                        ui.close();
                    }
                }
            });
            ui.end_row();
        }
    });
    for action in kind.actions {
        let button = ui.add_enabled(!app.action_running(), egui::Button::new(action.name));
        if button.clicked() {
            app.run_action(id, action.name);
        }
    }
}

/// The graph's inputs and the arguments given for them.
pub fn inputs(app: &mut App, ui: &mut Ui) {
    let inputs: Vec<String> = app.project.graph.inputs().into_iter().map(String::from).collect();
    if inputs.is_empty() {
        return;
    }
    ui.weak("inputs");
    egui::Grid::new("inputs").num_columns(2).show(ui, |ui| {
        for input in inputs {
            ui.label(&input);
            // Any parameter bound to the input tells what kind of value it takes.
            let kind = app.project.graph.nodes().find_map(|(_, node)| {
                let param = node.bindings.iter().find(|(_, bound)| **bound == input)?.0;
                Some(app.registry.get(&node.kind)?.param(param)?.kind)
            });
            let current = app.project.arguments().get(&input).cloned().unwrap_or(Json::Null);
            if let Some(kind) = kind
                && let Some(value) =
                    edit_value(ui, egui::Id::new(("input", &input)), &kind, &current)
                && let Err(e) = app.project.set_argument(&app.registry, &input, value)
            {
                app.report(Err(e.to_string()));
            }
            ui.end_row();
        }
    });
}

/// A widget for one parameter value; returns the new value when edited.
fn edit_value(ui: &mut Ui, id: egui::Id, kind: &ParamKind, value: &Json) -> Option<Json> {
    match *kind {
        ParamKind::Float { min, max, default } => {
            let mut v = value.as_f64().unwrap_or(default);
            ui.add(egui::Slider::new(&mut v, min..=max)).changed().then(|| json!(v))
        }
        ParamKind::Int { min, max, default } => {
            let mut v = value.as_i64().unwrap_or(default);
            ui.add(egui::DragValue::new(&mut v).range(min..=max)).changed().then(|| json!(v))
        }
        ParamKind::Bool { default } => {
            let mut v = value.as_bool().unwrap_or(default);
            ui.checkbox(&mut v, "").changed().then(|| json!(v))
        }
        ParamKind::Choice { options, default } => {
            let current = value.as_str().unwrap_or(default);
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

/// Each channel's counts per stop, scaled by the square root so that small
/// populations stay visible; the line marks 1.0 (0 EV).
pub fn histogram(ui: &mut Ui, h: &Histogram) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let peak = h.counts.iter().flatten().map(|&c| (c as f32).sqrt()).fold(1.0, f32::max);
    let x = |i: usize| rect.left() + rect.width() * i as f32 / (h.counts.len() - 1) as f32;
    let colors = [
        egui::Color32::from_rgb(110, 20, 20),
        egui::Color32::from_rgb(20, 80, 20),
        egui::Color32::from_rgb(20, 30, 110),
    ];
    for (c, color) in colors.into_iter().enumerate() {
        let points: Vec<Pos2> = h
            .counts
            .iter()
            .enumerate()
            .map(|(i, n)| {
                egui::pos2(x(i), rect.bottom() - rect.height() * (n[c] as f32).sqrt() / peak)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
    let zero = rect.left() + rect.width() * -h.min_stop / (h.max_stop - h.min_stop);
    painter.vline(zero, rect.y_range(), Stroke::new(1.0, theme::DARKER));
    let font = FontId::proportional(10.0);
    painter.text(
        rect.left_bottom(),
        Align2::LEFT_BOTTOM,
        format!("{} EV", h.min_stop),
        font.clone(),
        theme::WEAK,
    );
    painter.text(
        rect.right_bottom(),
        Align2::RIGHT_BOTTOM,
        format!("+{} EV", h.max_stop),
        font,
        theme::WEAK,
    );
}
