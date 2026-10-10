//! Node parameter panels and schema widgets, shared by the inspector and pop-outs.

use drip::param::ParamKind;
use egui::{PointerButton, Sense, Ui};
use serde_json::{Value as Json, json};

use crate::editing::NodeCx;
use crate::widgets::{dropdown, scroll_steps};

/// A node's name, kind, parameters and actions, without inspector help.
pub fn panel(ui: &mut Ui, cx: &mut NodeCx) {
    heading(ui, cx);
    controls(ui, cx);
}

pub fn heading(ui: &mut Ui, cx: &mut NodeCx) {
    let id = cx.id();
    let name_id = egui::Id::new(("name", id));
    if ui.data_mut(|data| data.remove_temp::<()>(name_id.with("focus"))).is_some() {
        ui.memory_mut(|memory| memory.request_focus(name_id));
    }
    if let Some(name) = edit_text(ui, name_id, &cx.node().name) {
        cx.set_name(&name);
    }
    ui.weak(cx.node().kind.id);
}

pub fn focus_name(ctx: &egui::Context, id: crate::model::NodeId) {
    // The inspector precedes the canvas; focus once the newly selected field exists.
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(("name", id)).with("focus"), ()));
    ctx.request_repaint();
}

pub fn controls(ui: &mut Ui, cx: &mut NodeCx) {
    let kind = cx.node().kind;
    crate::node_ui::of(kind).controls(ui, cx);
    if let Some(action) = crate::node_ui::binding(kind).and_then(|b| b.action_name())
        && ui.add_enabled(!cx.actions_disabled(), egui::Button::new(action)).clicked()
    {
        cx.run(action);
    }
}

/// Default schema controls, shared by custom node panels and generic nodes.
pub fn schema(ui: &mut Ui, cx: &mut NodeCx) {
    let node = cx.node();
    let (id, kind) = (cx.id(), node.kind);
    egui::Grid::new(("params", id)).num_columns(2).show(ui, |ui| {
        for spec in kind.params {
            let external = node.external.contains(spec.name);
            let text =
                if external { format!("{} (template)", spec.label) } else { spec.label.into() };
            // Generated from the parameter's doc comment; empty when undocumented.
            let hint = match spec.documentation {
                "" => "Right-click for reset and template options".to_owned(),
                doc => format!("{doc}\n\nRight-click for reset and template options"),
            };
            let name = ui.label(text).interact(Sense::click()).on_hover_text(hint);
            if let Some(value) = edit_parameter(ui, egui::Id::new((id, spec.name)), &node, spec) {
                cx.set_param(spec.name, value);
            }
            crate::theme::context_menu(&name).show(|ui| {
                if let Some(value) = reset_action(ui, &spec.kind) {
                    cx.set_param(spec.name, value);
                }
                ui.separator();
                let toggle = if external {
                    "Keep value in template"
                } else {
                    "Expose as template parameter"
                };
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
#[cfg(test)]
pub fn edit_value(ui: &mut Ui, id: egui::Id, kind: &ParamKind, value: &Json) -> Option<Json> {
    edit_value_with(ui, id, kind, value, "", &|_| super::ParameterUi::default())
}

pub fn edit_parameter(
    ui: &mut Ui,
    id: egui::Id,
    node: &crate::model::Node,
    spec: &drip::param::ParamSpec,
) -> Option<Json> {
    edit_value_with(ui, id, &spec.kind, &node.params[spec.name], spec.name, &|path| {
        super::binding(node.kind).map_or_else(super::ParameterUi::default, |binding| {
            binding.parameter_ui(node.params.clone(), path)
        })
    })
}

fn edit_value_with(
    ui: &mut Ui,
    id: egui::Id,
    kind: &ParamKind,
    value: &Json,
    path: &str,
    hints: &dyn Fn(&str) -> super::ParameterUi,
) -> Option<Json> {
    let id = ui.make_persistent_id(id);
    let (response, mut chosen) = match *kind {
        ParamKind::Float { min, max, .. } => {
            let mut v = value.as_f64().expect("validated float");
            let response = slider(ui, &mut v, min..=max);
            let steps = scroll_steps(ui, &response);
            if steps != 0.0 {
                v = (v + steps * (max - min) / 100.0).clamp(min, max);
            }
            (response, Some(json!(v)))
        }
        ParamKind::Int { min, max, .. } => {
            let mut v = value.as_i64().expect("validated int");
            let response = slider(ui, &mut v, min..=max);
            let steps = scroll_steps(ui, &response) as i64;
            if steps != 0 {
                v = v.saturating_add(steps).clamp(min, max);
            }
            (response, Some(json!(v)))
        }
        ParamKind::Bool { .. } => {
            let mut v = value.as_bool().expect("validated bool");
            let response = ui.checkbox(&mut v, "");
            (response, Some(json!(v)))
        }
        ParamKind::Choice { options, .. } => {
            ui.vertical(|ui| {
                let previous = options
                    .iter()
                    .position(|option| option.selected(value))
                    .expect("validated choice");
                let mut selected = previous;
                let response = dropdown(
                    ui,
                    egui::ComboBox::from_id_salt(id),
                    &mut selected,
                    &(0..options.len()).collect::<Vec<_>>(),
                    |index| options[index].label.into(),
                );
                let option = &options[selected];
                let mut edited = if selected == previous {
                    value.clone()
                } else {
                    ui.data_mut(|data| {
                        data.insert_temp(id.with(options[previous].name), value.clone())
                    });
                    ui.data(|data| data.get_temp::<Json>(id.with(option.name)))
                        .unwrap_or_else(|| option.default_value())
                };
                if let Some(parameters) = option.parameters {
                    ui.indent(id.with(option.name), |ui| {
                        egui::Grid::new(id.with((option.name, "fields"))).num_columns(2).show(
                            ui,
                            |ui| {
                                for spec in parameters {
                                    let label = ui.label(spec.label);
                                    if !spec.documentation.is_empty() {
                                        label.on_hover_text(spec.documentation);
                                    }
                                    let field_path =
                                        format!("{path}.{}.{}", option.name, spec.name);
                                    if let Some(value) = edit_value_with(
                                        ui,
                                        id.with((option.name, spec.name)),
                                        &spec.kind,
                                        &edited[option.name][spec.name],
                                        &field_path,
                                        hints,
                                    ) {
                                        edited[option.name][spec.name] = value;
                                    }
                                    ui.end_row();
                                }
                            },
                        );
                    });
                }
                (response, Some(edited))
            })
            .inner
        }
        ParamKind::Path { output } => {
            let selected_path = value.as_str().map(std::path::Path::new);
            let name = selected_path
                .and_then(|p| p.file_name())
                .map_or("Not selected".into(), |n| n.to_string_lossy());
            let mut chosen = None;
            let response = ui
                .horizontal(|ui| {
                    if ui.button("Browse…").clicked() {
                        let file = hints(path).file;
                        let mut dialog = rfd::FileDialog::new();
                        if let Some(file) = file {
                            dialog = dialog
                                .set_title(file.title)
                                .add_filter(file.filter, file.extensions);
                        }
                        let dialog = match selected_path.and_then(|p| p.parent()) {
                            Some(dir) => dialog.set_directory(dir),
                            None => dialog,
                        };
                        chosen = if output { dialog.save_file() } else { dialog.pick_file() }.map(
                            |mut p| {
                                if output
                                    && p.extension().is_none()
                                    && let Some(extension) = file.and_then(|f| f.extensions.first())
                                {
                                    p.set_extension(extension);
                                }
                                json!(p)
                            },
                        );
                    }
                    let label = ui.add(egui::Label::new(name).sense(Sense::click()));
                    if let Some(path) = selected_path {
                        label.clone().on_hover_text(path.display().to_string());
                    }
                    label
                })
                .inner;
            (response, chosen)
        }
    };
    let response = response.on_hover_text("Right-click to reset to default");
    let mut menu = crate::theme::context_menu(&response).id(ui.make_persistent_id((id, "reset")));
    // Slider rails sense drags, so their response does not report clicks.
    if response.hovered()
        && ui.input(|input| input.pointer.button_clicked(PointerButton::Secondary))
    {
        menu = menu.open_memory(egui::SetOpenCommand::Bool(true));
    }
    menu.show(|ui| {
        if let Some(value) = reset_action(ui, kind) {
            chosen = Some(value);
            response.surrender_focus();
        }
    });
    chosen.filter(|chosen| chosen != value)
}

fn slider<T: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
) -> egui::Response {
    let secondary = ui.input(|input| {
        input.pointer.button_down(PointerButton::Secondary)
            || input.pointer.button_released(PointerButton::Secondary)
    });
    let slider = egui::Slider::from_get_set(range.start().to_f64()..=range.end().to_f64(), |new| {
        // Reject right-button edits before egui reads the value back for painting.
        if !secondary && let Some(new) = new {
            *value = T::from_f64(new);
        }
        value.to_f64()
    });
    let slider = slider.clamping(egui::SliderClamping::Edits);
    ui.add(if T::INTEGRAL { slider.integer() } else { slider })
}

fn reset_action(ui: &mut Ui, kind: &ParamKind) -> Option<Json> {
    ui.button("Reset to default").clicked().then(|| {
        ui.close();
        kind.default_value()
    })
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

#[cfg(test)]
mod tests;
