//! The node editor (DESIGN G1). It draws straight from the project's graph and
//! edits it only through the graph's validated operations; node positions live
//! in each node's opaque `ui` field.

use drip::eval::{NodeError, NodeResult};
use drip::graph::{NodeId, Port};
use drip::node::Registry;
use drip::project::Project;
use egui::epaint::CubicBezierShape;
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;

use crate::theme;

const WIDTH: f32 = 160.0;
const HEADER: f32 = 22.0;
const ROW: f32 = 18.0;
const PORT: f32 = 4.0;

#[derive(Default)]
pub struct Editor {
    /// Pan offset of the graph's origin within the editor; `None` until the
    /// graph has been fitted into view.
    offset: Option<Vec2>,
    /// The output a wire is being dragged from.
    wire: Option<Port>,
}

/// Where a node and its ports are on screen.
struct Layout {
    id: NodeId,
    rect: Rect,
    inputs: Vec<(&'static str, Pos2)>,
    outputs: Vec<(&'static str, Pos2)>,
    error: Option<String>,
}

impl Editor {
    /// Shows the editor; returns an error to report if an edit was refused.
    pub fn show<'a>(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        registry: &Registry,
        selected: &mut Option<NodeId>,
        results: impl Fn(NodeId) -> Option<&'a NodeResult>,
    ) -> Option<String> {
        let (area, background) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let offset = self.offset.get_or_insert_with(|| {
            let positions: Vec<_> =
                project.graph.nodes().map(|(id, node)| position(&node.ui, id).to_pos2()).collect();
            if positions.is_empty() {
                return Vec2::ZERO;
            }
            let center =
                Rect::from_points(&positions).center().to_vec2() + vec2(WIDTH, HEADER) / 2.0;
            area.size() / 2.0 - center
        });
        if background.dragged() {
            *offset += background.drag_delta();
        }
        if background.clicked() {
            *selected = None;
        }
        let origin = area.min + *offset;
        background.context_menu(|ui| {
            for kind in registry.kinds() {
                if ui.button(kind.name).clicked() {
                    let at = ui.ctx().pointer_interact_pos().unwrap_or(area.center()) - origin;
                    let id = project.graph.add_node(kind);
                    project.graph.set_ui(id, json!({ "pos": [at.x, at.y] })).expect("just added");
                    *selected = Some(id);
                    ui.close();
                }
            }
        });

        let layouts: Vec<Layout> = project
            .graph
            .nodes()
            .map(|(id, node)| {
                let pos = origin + position(&node.ui, id);
                let kind = registry.get(&node.kind);
                let inputs = kind.map_or(&[][..], |k| k.inputs).iter().map(|p| p.name);
                let outputs = kind.map_or(&[][..], |k| k.outputs).iter().map(|p| p.name);
                let rows = inputs.len().max(outputs.len());
                let port =
                    |i: usize, x: f32| pos2(pos.x + x, pos.y + HEADER + ROW * (i as f32 + 0.5));
                let error = match results(id) {
                    Some(Err(NodeError::Upstream(_))) | Some(Ok(_)) | None => None,
                    Some(Err(e)) => Some(e.to_string()),
                };
                let height = HEADER + ROW * (rows + usize::from(error.is_some())) as f32 + PORT;
                Layout {
                    id,
                    rect: Rect::from_min_size(pos, vec2(WIDTH, height)),
                    inputs: inputs.enumerate().map(|(i, name)| (name, port(i, 0.0))).collect(),
                    outputs: outputs.enumerate().map(|(i, name)| (name, port(i, WIDTH))).collect(),
                    error,
                }
            })
            .collect();
        let port_pos = |port: &Port, outputs: bool| {
            let layout = layouts.iter().find(|l| l.id == port.0)?;
            let ports = if outputs { &layout.outputs } else { &layout.inputs };
            ports.iter().find(|(name, _)| *name == port.1).map(|(_, pos)| *pos)
        };

        let wire = Stroke::new(1.5, theme::TEXT);
        for (output, input) in project.graph.edges() {
            if let (Some(from), Some(to)) = (port_pos(output, true), port_pos(input, false)) {
                painter.add(bezier(from, to, wire));
            }
        }

        let mut refused = None;
        for layout in &layouts {
            let node = project.graph.node(layout.id).expect("laid out from the graph");
            let body = ui.interact(layout.rect, ui.id().with(layout.id), Sense::click_and_drag());
            if body.clicked() || body.drag_started() {
                *selected = Some(layout.id);
            }
            if body.dragged() {
                let pos = layout.rect.min - origin + body.drag_delta();
                let mut ui_state = node.ui.clone();
                if !ui_state.is_object() {
                    ui_state = json!({});
                }
                ui_state["pos"] = json!([pos.x, pos.y]);
                project.graph.set_ui(layout.id, ui_state).expect("node exists");
            }
            let node = project.graph.node(layout.id).expect("laid out from the graph");
            let fill = if *selected == Some(layout.id) { theme::LIGHTER } else { theme::DARKER };
            painter.rect_filled(layout.rect, 0.0, fill);
            let font = FontId::proportional(13.0);
            painter.text(
                layout.rect.min + vec2(8.0, HEADER / 2.0),
                Align2::LEFT_CENTER,
                &node.label,
                font.clone(),
                theme::TEXT,
            );
            let small = FontId::proportional(11.0);
            for (name, pos) in &layout.inputs {
                painter.circle_filled(*pos, PORT, theme::TEXT);
                painter.text(
                    *pos + vec2(8.0, 0.0),
                    Align2::LEFT_CENTER,
                    name,
                    small.clone(),
                    theme::WEAK,
                );
                let port = ui.interact(
                    Rect::from_center_size(*pos, Vec2::splat(4.0 * PORT)),
                    ui.id().with((layout.id, name, 0)),
                    Sense::click(),
                );
                if port.secondary_clicked() {
                    project.graph.disconnect(&Port(layout.id, (*name).into()));
                }
                port.on_hover_text("right-click to disconnect");
            }
            for (name, pos) in &layout.outputs {
                painter.circle_filled(*pos, PORT, theme::TEXT);
                painter.text(
                    *pos - vec2(8.0, 0.0),
                    Align2::RIGHT_CENTER,
                    name,
                    small.clone(),
                    theme::WEAK,
                );
                let port = ui.interact(
                    Rect::from_center_size(*pos, Vec2::splat(4.0 * PORT)),
                    ui.id().with((layout.id, name, 1)),
                    Sense::drag(),
                );
                if port.drag_started() {
                    self.wire = Some(Port(layout.id, (*name).into()));
                }
            }
            if let Some(error) = &layout.error {
                let at = pos2(layout.rect.min.x + 8.0, layout.rect.max.y - PORT - ROW / 2.0);
                painter.text(at, Align2::LEFT_CENTER, elide(error, 26), small, theme::ERROR);
                let row =
                    Rect::from_center_size(pos2(layout.rect.center().x, at.y), vec2(WIDTH, ROW));
                ui.interact(row, ui.id().with((layout.id, "error")), Sense::hover())
                    .on_hover_text(error);
            }
        }

        if let Some(from) = self.wire.clone() {
            let pointer = ui.ctx().pointer_latest_pos();
            if let (Some(start), Some(end)) = (port_pos(&from, true), pointer) {
                painter.add(bezier(start, end, wire));
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.wire = None;
                let target = pointer.and_then(|p| {
                    layouts
                        .iter()
                        .flat_map(|l| l.inputs.iter().map(move |(name, pos)| (l.id, *name, *pos)))
                        .find(|(_, _, pos)| pos.distance(p) <= 2.0 * PORT)
                });
                if let Some((id, name, _)) = target
                    && let Err(e) = project.graph.connect(registry, from, Port(id, name.into()))
                {
                    refused = Some(e.to_string());
                }
            }
        }

        if let Some(id) = *selected
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            project.graph.remove_node(id);
            *selected = None;
        }
        refused
    }
}

/// A node's saved position, or a spot derived from its id for nodes never placed.
fn position(ui: &serde_json::Value, id: NodeId) -> Vec2 {
    match ui["pos"]
        .as_array()
        .map(|p| p.iter().filter_map(|v| v.as_f64()).collect::<Vec<_>>())
        .as_deref()
    {
        Some([x, y]) => vec2(*x as f32, *y as f32),
        _ => vec2(40.0 + 200.0 * (id.0 % 5) as f32, 40.0 + 140.0 * (id.0 / 5) as f32),
    }
}

fn bezier(from: Pos2, to: Pos2, stroke: Stroke) -> CubicBezierShape {
    let bend = vec2(((to.x - from.x).abs() / 2.0).max(40.0), 0.0);
    CubicBezierShape::from_points_stroke(
        [from, from + bend, to - bend, to],
        false,
        egui::Color32::TRANSPARENT,
        stroke,
    )
}

fn elide(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        text.into()
    } else {
        text.chars().take(chars - 1).chain(['…']).collect()
    }
}
