//! Photo workspace over the same graph and prepared views as the node editor.
use crate::{
    editing::{Frame, NodeCx},
    inspector,
    model::{Graph, NodeId},
    node_ui::{self, parameters},
    theme,
};
use egui::{FontId, RichText, Ui};

#[derive(Default, PartialEq)]
pub enum Sidebar {
    #[default]
    Adjust,
    Output,
    Inspect,
}

#[derive(Default)]
pub struct Workspace {
    pub nodes: bool,
    pub pipeline: bool,
    pub sidebar: Sidebar,
    preview: Option<NodeId>,
    scope: Option<NodeId>,
}

impl Workspace {
    pub fn sidebar(
        &mut self,
        ui: &mut Ui,
        graph: &mut Graph,
        selected: Option<NodeId>,
        frame: &mut Frame,
    ) {
        if self.nodes {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(id) = selected.filter(|id| graph.node(*id).is_some()) {
                    inspector::selected_node(ui, &mut NodeCx::new(graph, id, frame));
                    ui.separator();
                }
                inspector::inputs(ui, graph, frame);
            });
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.sidebar, Sidebar::Adjust, "Adjust");
            ui.selectable_value(&mut self.sidebar, Sidebar::Output, "Export settings");
            ui.selectable_value(&mut self.sidebar, Sidebar::Inspect, "Inspector");
        });
        ui.add_space(8.0);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("workspace controls").show(ui, |ui| {
            if self.sidebar == Sidebar::Inspect {
                if let Some(id) = selected.filter(|id| graph.node(*id).is_some()) {
                    inspector::selected_node(ui, &mut NodeCx::new(graph, id, frame));
                } else {
                    ui.weak(
                        "Select a node in the processing graph to inspect its ports and settings.",
                    );
                }
                inspector::inputs(ui, graph, frame);
                return;
            }
            if self.sidebar == Sidebar::Adjust {
                self.scopes(ui, graph, frame);
            }
            let mut ids: Vec<_> = graph
                .nodes()
                .filter_map(|(id, node)| {
                    let show = if self.sidebar == Sidebar::Output {
                        node.kind.category == "Output"
                    } else {
                        node.kind.category != "Output"
                            && node.kind.category != "View"
                            && !node.kind.params.is_empty()
                    };
                    show.then_some(id)
                })
                .collect();
            ids.sort_by_key(|id| match graph.node(*id).expect("listed node").kind.category {
                "Tone" => 0,
                "Color" => 1,
                "Geometry" => 2,
                "Sensor" => 3,
                _ => 4,
            });
            if ids.is_empty() {
                ui.weak(if self.sidebar == Sidebar::Output {
                    "Add an Export TIFF node in the node graph to export this project."
                } else {
                    "This graph has no adjustable processing nodes."
                });
            }
            for id in ids {
                let node = graph.node(id).expect("listed node");
                ui.add_space(10.0);
                egui::CollapsingHeader::new(RichText::new(&node.name).strong())
                    .id_salt(("adjustment", id))
                    .default_open(matches!(node.kind.category, "Tone" | "Output"))
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        parameters::controls(ui, &mut NodeCx::new(graph, id, frame));
                        ui.add_space(6.0);
                    });
                ui.separator();
            }
        });
    }

    fn scopes(&mut self, ui: &mut Ui, graph: &Graph, frame: &Frame) {
        let scopes: Vec<_> = graph
            .nodes()
            .filter(|(_, n)| {
                n.kind.category == "View"
                    && n.kind.id != "view.preview"
                    && node_ui::binding(n.kind).is_some_and(|b| b.has_preparation())
            })
            .map(|(id, node)| (id, node.name))
            .collect();
        choose(ui, "Scope", &scopes, &mut self.scope);
        let Some(id) = self.scope else { return };
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 165.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, theme::DARKER);
        if let Some(Ok(Some(view))) = (frame.results)(id) {
            view.draw(
                ui.painter(),
                rect.shrink(8.0),
                ui.id().with("scope"),
                &FontId::proportional(11.0),
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Scope available after processing",
                FontId::proportional(12.0),
                theme::WEAK,
            );
        }
        ui.add_space(4.0);
    }

    /// Returns a request for the application's RAW picker.
    pub fn photo(&mut self, ui: &mut Ui, graph: &mut Graph, frame: &mut Frame, busy: bool) -> bool {
        let previews: Vec<_> = graph
            .nodes()
            .filter(|(_, n)| n.kind.id == "view.preview")
            .map(|(id, n)| (id, n.name))
            .collect();
        let mut open = false;
        ui.horizontal(|ui| {
            choose(ui, "Preview", &previews, &mut self.preview);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(id) = self.preview {
                    let mut cx = NodeCx::new(graph, id, frame);
                    if ui.small_button("Pop out").clicked() {
                        cx.toggle(node_ui::Part::View);
                    }
                }
                ui.toggle_value(&mut self.pipeline, "Show pipeline");
            });
        });
        ui.separator();
        if let Some(id) = self.preview {
            egui::CollapsingHeader::new("Preview options").id_salt("preview options").show(
                ui,
                |ui| {
                    parameters::controls(ui, &mut NodeCx::new(graph, id, frame));
                },
            );
            if let Some(Ok(Some(view))) = (frame.results)(id) {
                ui.push_id(("photo", id), |ui| view.window(ui));
                return false;
            }
        }
        let has_raw = graph.nodes().any(|(_, n)| n.kind.id == "input.bayer-raw");
        let has_source = graph
            .nodes()
            .any(|(_, n)| n.kind.id == "input.bayer-raw" && n.params["path"].as_str().is_some());
        ui.add_space((ui.available_height() * 0.32).max(0.0));
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(if has_raw {
                    "Open a photograph"
                } else {
                    "Build your image pipeline"
                })
                .size(28.0)
                .color(theme::TEXT),
            );
            ui.add_space(12.0);
            if busy {
                ui.weak("Preparing your preview…");
            } else if has_source {
                if let Some(Err(error)) = self.preview.and_then(|id| (frame.results)(id)) {
                    ui.colored_label(theme::ERROR, error.to_string());
                } else {
                    ui.weak("Connect a Preview node to an image in the processing graph.");
                }
            } else if has_raw {
                ui.weak("Open a Bayer RAW to begin. Your adjustments stay editable.");
                ui.add_space(20.0);
                open = ui.add_sized([160.0, 38.0], egui::Button::new("Choose RAW…")).clicked();
            } else {
                ui.weak("Add and connect a Preview node in the node graph.");
                if ui.button("Open node graph").clicked() {
                    self.nodes = true;
                }
            }
        });
        open
    }
}

fn choose(ui: &mut Ui, label: &str, choices: &[(NodeId, String)], selected: &mut Option<NodeId>) {
    if !choices.iter().any(|(id, _)| Some(*id) == *selected) {
        *selected = choices.first().map(|(id, _)| *id);
    }
    let name =
        choices.iter().find(|(id, _)| Some(*id) == *selected).map_or("None", |(_, name)| name);
    egui::ComboBox::from_id_salt(label)
        .selected_text(name)
        .show_ui(ui, |ui| {
            for (id, name) in choices {
                ui.selectable_value(selected, Some(*id), name);
            }
        })
        .response
        .on_hover_text(label);
}
