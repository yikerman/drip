//! The node editor (DESIGN G1, G7, G13): the main canvas. It draws straight from
//! the graph and edits it only through the graph's validated operations. Node
//! positions and sizes live in each node's opaque `ui` field, in graph units.
//! The canvas is an egui layer that egui transforms to pan and zoom, so
//! everything on it is drawn and interacted with in graph units. Nodes whose
//! result has a view draw it in their body and can be resized.

use crate::views::PreparedView;
use crate::worker::Presentation;
use drip::eval::NodeError;
use drip::graph::{Graph, Node, NodeId, Port};
use drip::node::Registry;
use egui::emath::TSTransform;
use egui::epaint::CubicBezierShape;
use egui::{Align2, FontId, LayerId, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value as Json, json};

use crate::{theme, views};

const WIDTH: f32 = 160.0;
const HEADER: f32 = 22.0;
const ROW: f32 = 18.0;
const PORT: f32 = 4.0;
const PAD: f32 = 6.0;
/// Default size of a node's view, in graph units.
const VIEW: Vec2 = vec2(280.0, 190.0);
/// egui rasterizes text once and scales it with the layer, so zooming in past
/// 1 would blur it.
const ZOOM: (f32, f32) = (0.2, 1.0);
/// Space kept around the graph when fitting it into view, in screen points.
const MARGIN: f32 = 20.0;

pub struct Editor {
    /// Screen offset of the graph's origin within the canvas; `None` until
    /// the graph has been fitted into view.
    offset: Option<Vec2>,
    zoom: f32,
    /// The output a wire is being dragged from.
    wire: Option<Port>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor { offset: None, zoom: 1.0, wire: None }
    }
}

/// What one frame of the editor reads from evaluation and reports back.
pub struct Frame<'a> {
    pub results: &'a dyn Fn(NodeId) -> Option<&'a Presentation>,
    /// An edit the graph refused.
    pub refused: Option<String>,
    pub changed: bool,
}

/// Where a node and its parts are on screen.
struct Layout {
    id: NodeId,
    rect: Rect,
    inputs: Vec<(&'static str, Pos2)>,
    outputs: Vec<(&'static str, Pos2)>,
    error: Option<(Pos2, String)>,
    view: Option<(Rect, PreparedView)>,
}

impl Editor {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        graph: &mut Graph,
        registry: &Registry,
        selected: &mut Option<NodeId>,
        frame: &mut Frame,
    ) {
        let (area, background) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let offset = self.offset.get_or_insert_with(|| {
            let (offset, zoom) = fit(graph, area);
            self.zoom = zoom;
            offset
        });
        if background.dragged() {
            *offset += background.drag_delta();
        }
        if background.clicked() {
            *selected = None;
        }
        // The wheel zooms around the pointer, keeping the point under it fixed.
        if let Some(pointer) = ui.ctx().pointer_hover_pos().filter(|p| area.contains(*p)) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let zoom = (self.zoom * (scroll * 0.002).exp()).clamp(ZOOM.0, ZOOM.1);
                let at = pointer - area.min;
                *offset = at - (at - *offset) * (zoom / self.zoom);
                self.zoom = zoom;
            }
        }
        let zoom = self.zoom;
        let to_global = TSTransform::new(area.min.to_vec2() + *offset, zoom);
        background.context_menu(|ui| {
            for kind in registry.kinds() {
                if ui.button(kind.label).clicked() {
                    let pointer = ui.ctx().pointer_interact_pos().unwrap_or(area.center());
                    let id = graph.add_node(kind);
                    frame.changed = true;
                    set_ui(graph, id, "pos", (to_global.inverse() * pointer).to_vec2());
                    *selected = Some(id);
                    ui.close();
                }
            }
        });

        let layer = LayerId::new(ui.layer_id().order, ui.id().with("canvas"));
        ui.ctx().set_sublayer(ui.layer_id(), layer);
        ui.ctx().set_transform_layer(layer, to_global);
        let visible = to_global.inverse() * area;
        let ui = &mut ui.new_child(UiBuilder::new().layer_id(layer).max_rect(visible));
        ui.set_clip_rect(visible);
        let painter = ui.painter().clone();

        let results = frame.results;
        let layouts: Vec<Layout> =
            graph.nodes().map(|(id, node)| layout(id, node, results(id))).collect();
        let port_pos = |port: &Port, outputs: bool| {
            let layout = layouts.iter().find(|l| l.id == port.0)?;
            let ports = if outputs { &layout.outputs } else { &layout.inputs };
            ports.iter().find(|(name, _)| *name == port.1).map(|(_, pos)| *pos)
        };

        // Strokes and hit areas are in graph units; these keep their on-screen
        // size as it was before the canvas was transformed.
        let wire = Stroke::new(1.5 / zoom.sqrt(), theme::TEXT);
        for (output, input) in graph.edges() {
            if let (Some(from), Some(to)) = (port_pos(output, true), port_pos(input, false)) {
                painter.add(bezier(from, to, wire));
            }
        }

        let (font, small) = (FontId::proportional(13.0), FontId::proportional(11.0));
        let hit = |pos: Pos2| Rect::from_center_size(pos, Vec2::splat(4.0 * PORT / zoom));
        for l in &layouts {
            let body = ui.interact(l.rect, ui.id().with(l.id), Sense::click_and_drag());
            if body.clicked() || body.drag_started() {
                *selected = Some(l.id);
            }
            if body.dragged() {
                let pos =
                    position(&graph.node(l.id).expect("laid out").ui, l.id) + body.drag_delta();
                set_ui(graph, l.id, "pos", pos);
            }
            let node = graph.node(l.id).expect("laid out from the graph");
            let saved_size = view_size(&node.ui);
            let fill = if *selected == Some(l.id) { theme::LIGHTER } else { theme::DARKER };
            painter.rect_filled(l.rect, 0.0, fill);
            let title = l.rect.min + vec2(8.0, HEADER / 2.0);
            painter.text(title, Align2::LEFT_CENTER, &node.label, font.clone(), theme::TEXT);
            for (name, pos) in &l.inputs {
                painter.circle_filled(*pos, PORT, theme::TEXT);
                painter.text(
                    *pos + vec2(8.0, 0.0),
                    Align2::LEFT_CENTER,
                    name,
                    small.clone(),
                    theme::WEAK,
                );
                let port = ui.interact(hit(*pos), ui.id().with((l.id, name, 0)), Sense::click());
                if port.secondary_clicked() {
                    graph.disconnect(&Port(l.id, (*name).into()));
                    frame.changed = true;
                }
                port.on_hover_text("right-click to disconnect");
            }
            for (name, pos) in &l.outputs {
                painter.circle_filled(*pos, PORT, theme::TEXT);
                painter.text(
                    *pos - vec2(8.0, 0.0),
                    Align2::RIGHT_CENTER,
                    name,
                    small.clone(),
                    theme::WEAK,
                );
                if ui
                    .interact(hit(*pos), ui.id().with((l.id, name, 1)), Sense::drag())
                    .drag_started()
                {
                    self.wire = Some(Port(l.id, (*name).into()));
                }
            }
            if let Some((at, error)) = &l.error {
                painter.text(
                    *at,
                    Align2::LEFT_CENTER,
                    elide(error, 26),
                    small.clone(),
                    theme::ERROR,
                );
                let row = Rect::from_min_size(
                    pos2(l.rect.min.x, at.y - ROW / 2.0),
                    vec2(l.rect.width(), ROW),
                );
                ui.interact(row, ui.id().with((l.id, "error")), Sense::hover())
                    .on_hover_text(error);
            }
            if let Some((rect, view)) = &l.view {
                views::draw(&painter, *rect, ui.id().with((l.id, "view")), view, &small);
                let corner = Rect::from_min_max(l.rect.max - Vec2::splat(10.0), l.rect.max);
                painter.line_segment(
                    [corner.left_bottom(), corner.right_top()],
                    Stroke::new(1.0 / zoom, theme::WEAK),
                );
                let handle = ui.interact(corner, ui.id().with((l.id, "resize")), Sense::drag());
                if handle.dragged() {
                    let size = (saved_size + handle.drag_delta()).max(vec2(80.0, 60.0));
                    set_ui(graph, l.id, "size", size);
                }
            }
        }

        if let Some(from) = self.wire.clone() {
            let pointer = ui.ctx().pointer_latest_pos().map(|p| to_global.inverse() * p);
            if let (Some(start), Some(end)) = (port_pos(&from, true), pointer) {
                painter.add(bezier(start, end, wire));
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.wire = None;
                let mut inputs = layouts
                    .iter()
                    .flat_map(|l| l.inputs.iter().map(move |(name, pos)| (l.id, *name, *pos)));
                let target = pointer.and_then(|p| inputs.find(|(_, _, pos)| hit(*pos).contains(p)));
                if let Some((id, name, _)) = target {
                    match graph.connect(from, Port(id, name.into())) {
                        Ok(()) => frame.changed = true,
                        Err(e) => frame.refused = Some(e.to_string()),
                    }
                }
            }
        }

        if let Some(id) = *selected
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            graph.remove_node(id);
            frame.changed = true;
            *selected = None;
        }
    }
}

/// From the top: header, one row per port pair, an error row if the node
/// failed, the view if it has one.
fn layout(id: NodeId, node: &Node, result: Option<&Presentation>) -> Layout {
    let view = result.and_then(|r| r.as_ref().ok()).and_then(Clone::clone);
    let error = match result {
        Some(Err(NodeError::Upstream(_))) | Some(Ok(_)) | None => None,
        Some(Err(e)) => Some(e.to_string()),
    };
    let size = if view.is_some() { view_size(&node.ui) } else { vec2(WIDTH, 0.0) };
    let ports = HEADER + ROW * node.kind.inputs.len().max(node.kind.outputs.len()) as f32;
    let view_top = ports + if error.is_some() { ROW } else { 0.0 };
    let pos = position(&node.ui, id).to_pos2();
    let port = |i: usize, x: f32| pos + vec2(x, HEADER + ROW * (i as f32 + 0.5));
    Layout {
        id,
        rect: Rect::from_min_size(pos, vec2(size.x, view_top + size.y + PAD)),
        inputs: node.kind.inputs.iter().enumerate().map(|(i, p)| (p.name, port(i, 0.0))).collect(),
        outputs: node
            .kind
            .outputs
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name, port(i, size.x)))
            .collect(),
        error: error.map(|e| (pos + vec2(8.0, ports + ROW / 2.0), e)),
        view: view.map(|v| {
            (Rect::from_min_size(pos + vec2(PAD, view_top), size - vec2(2.0 * PAD, 0.0)), v)
        }),
    }
}

/// A node's saved position, or a spot derived from its id for nodes never placed.
fn position(ui: &Json, id: NodeId) -> Vec2 {
    pair(&ui["pos"])
        .unwrap_or_else(|| vec2(40.0 + 200.0 * (id.0 % 5) as f32, 40.0 + 140.0 * (id.0 / 5) as f32))
}

/// The saved size of a node that has a view, in graph units.
fn view_size(ui: &Json) -> Vec2 {
    pair(&ui["size"]).unwrap_or(VIEW)
}

fn pair(value: &Json) -> Option<Vec2> {
    match value.as_array()?.iter().map(|v| v.as_f64()).collect::<Option<Vec<_>>>()?.as_slice() {
        [x, y] => Some(vec2(*x as f32, *y as f32)),
        _ => None,
    }
}

/// Sets `key` in a node's ui object, keeping the rest of it.
fn set_ui(graph: &mut Graph, id: NodeId, key: &str, value: Vec2) {
    let mut ui = graph.node(id).expect("node exists").ui.clone();
    if !ui.is_object() {
        ui = json!({});
    }
    ui[key] = json!([value.x, value.y]);
    graph.set_ui(id, ui).expect("node exists");
}

/// A node's rectangle in graph units, with room for a view when it has a
/// saved size.
fn bounds(id: NodeId, node: &Node) -> Rect {
    let ports = HEADER + ROW * node.kind.inputs.len().max(node.kind.outputs.len()) as f32;
    let size = pair(&node.ui["size"]).unwrap_or(vec2(WIDTH, 0.0));
    Rect::from_min_size(position(&node.ui, id).to_pos2(), vec2(size.x, ports + size.y + PAD))
}

/// The offset and zoom that center the whole graph in `area`, zooming out as
/// far as needed to show all of it but never in.
fn fit(graph: &Graph, area: Rect) -> (Vec2, f32) {
    let Some(bounds) = graph.nodes().map(|(id, node)| bounds(id, node)).reduce(Rect::union) else {
        return (Vec2::ZERO, 1.0);
    };
    let room = area.size() - Vec2::splat(2.0 * MARGIN);
    let zoom = (room / bounds.size()).min_elem().clamp(ZOOM.0, 1.0);
    (area.size() / 2.0 - bounds.center().to_vec2() * zoom, zoom)
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
