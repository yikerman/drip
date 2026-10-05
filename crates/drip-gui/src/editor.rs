//! The node editor (DESIGN G1, G7, G13): the main canvas. It draws straight from
//! the graph and edits it only through the graph's validated operations. Node
//! positions and sizes live in each node's opaque `ui` field, in graph units.
//! The canvas is an egui layer that egui transforms to pan and zoom, so
//! everything on it is drawn and interacted with in graph units. The editor
//! draws each node's frame (header, ports, error) and its kind's GUI the body.

use crate::gui::{self, Frame, NodeCx, Part, pair, set_ui};
use crate::widgets::{self, BUTTON};
use crate::worker::Presentation;
use drip::eval::NodeError;
use drip::graph::{Graph, Node, NodeId, Port};
use drip::node::Registry;
use egui::emath::TSTransform;
use egui::epaint::CubicBezierShape;
use egui::{
    Align2, FontId, LayerId, Painter, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use serde_json::Value as Json;

use crate::theme;

const HEADER: f32 = 22.0;
const ROW: f32 = 18.0;
const PORT: f32 = 4.0;
/// egui rasterizes text once and scales it with the layer, so text blurs past
/// 1; previews draw at their own resolution and stay sharp.
const ZOOM: (f32, f32) = (0.2, 2.0);
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

/// Where a node and its parts are on screen.
struct Layout {
    id: NodeId,
    rect: Rect,
    inputs: Vec<(&'static str, Pos2)>,
    outputs: Vec<(&'static str, Pos2)>,
    error: Option<(Pos2, String)>,
    body: Rect,
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
        let (mut canvas, to_global) = self.canvas(ui, graph, registry, selected, frame);
        let zoom = to_global.scaling;
        let ui = &mut canvas;
        let painter = ui.painter().clone();

        let results = frame.results;
        let layouts: Vec<Layout> =
            graph.nodes().map(|(id, node)| layout(id, node, results(id))).collect();
        let port_pos = |port: &Port, outputs: bool| {
            let layout = layouts.iter().find(|l| l.id == port.0)?;
            let ports = if outputs { &layout.outputs } else { &layout.inputs };
            ports.iter().find(|(name, _)| *name == port.1).map(|(_, pos)| *pos)
        };

        // Wires thin out slower than the zoom; port hit areas keep their
        // on-screen size.
        let point = widgets::point(ui);
        let wire = Stroke::new(1.5 * zoom.sqrt() * point, theme::TEXT);
        for (output, input) in graph.edges() {
            if let (Some(from), Some(to)) = (port_pos(output, true), port_pos(input, false)) {
                painter.add(bezier(from, to, wire));
            }
        }

        let hit = |pos: Pos2| Rect::from_center_size(pos, Vec2::splat(16.0 * point));
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
            let (kind, kind_params) = (gui::of(node.kind), node.kind.params);
            paint(&painter, l, &node.label, *selected == Some(l.id));
            for (name, pos) in &l.inputs {
                let port = ui.interact(hit(*pos), ui.id().with((l.id, name, 0)), Sense::click());
                if port.secondary_clicked() {
                    graph.disconnect(&Port(l.id, (*name).into()));
                    frame.report.edited = true;
                }
                port.on_hover_text("right-click to disconnect");
            }
            for (name, pos) in &l.outputs {
                if ui
                    .interact(hit(*pos), ui.id().with((l.id, name, 1)), Sense::drag())
                    .drag_started()
                {
                    self.wire = Some(Port(l.id, (*name).into()));
                }
            }
            if let Some((at, error)) = &l.error {
                let row = Rect::from_min_size(
                    pos2(l.rect.min.x, at.y - ROW / 2.0),
                    vec2(l.rect.width(), ROW),
                );
                ui.interact(row, ui.id().with((l.id, "error")), Sense::hover())
                    .on_hover_text(error);
            }
            let mut cx = NodeCx::new(graph, l.id, frame);
            if !kind_params.is_empty() {
                let button = Rect::from_min_size(
                    l.rect.right_top() + vec2(-HEADER, (HEADER - BUTTON) / 2.0),
                    Vec2::splat(BUTTON),
                );
                widgets::pop_out(ui, button, &mut cx, Part::Parameters);
            }
            kind.body(&mut ui.new_child(UiBuilder::new().id_salt(l.id).max_rect(l.body)), &mut cx);
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
                        Ok(()) => frame.report.edited = true,
                        Err(e) => frame.report.refused = Some(e.to_string()),
                    }
                }
            }
        }

        if let Some(id) = *selected
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            graph.remove_node(id);
            frame.report.edited = true;
            *selected = None;
        }
    }

    /// The canvas: a layer over the editor's area under the current pan and
    /// zoom, and the transform into it. Its background pans, zooms, deselects
    /// and adds nodes.
    fn canvas(
        &mut self,
        ui: &mut Ui,
        graph: &mut Graph,
        registry: &Registry,
        selected: &mut Option<NodeId>,
        frame: &mut Frame,
    ) -> (Ui, TSTransform) {
        let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let offset = self.offset.get_or_insert_with(|| {
            let (offset, zoom) = fit(graph, area);
            self.zoom = zoom;
            offset
        });
        // The canvas is its own background, so the nodes drawn on it later
        // take input first; a widget behind its layer would get none.
        let layer = LayerId::new(ui.layer_id().order, ui.id().with("canvas"));
        ui.ctx().set_sublayer(ui.layer_id(), layer);
        let to_global = TSTransform::new(area.min.to_vec2() + *offset, self.zoom);
        let canvas = UiBuilder::new()
            .layer_id(layer)
            .max_rect(to_global.inverse() * area)
            .sense(Sense::click_and_drag());
        let mut canvas = ui.new_child(canvas);
        let background = canvas.response();
        if background.dragged() {
            *offset += ui.input(|i| i.pointer.delta());
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
        let to_global = TSTransform::new(area.min.to_vec2() + *offset, self.zoom);
        background.context_menu(|ui| {
            for kind in registry.kinds() {
                if ui.button(kind.label).clicked() {
                    let pointer = ui.ctx().pointer_interact_pos().unwrap_or(area.center());
                    let id = graph.add_node(kind);
                    frame.report.edited = true;
                    set_ui(graph, id, "pos", (to_global.inverse() * pointer).to_vec2());
                    *selected = Some(id);
                    ui.close();
                }
            }
        });

        ui.ctx().set_transform_layer(layer, to_global);
        let visible = to_global.inverse() * area;
        canvas.set_clip_rect(visible);
        canvas.expand_to_include_rect(visible);
        (canvas, to_global)
    }
}

/// Paints a node's frame: its fill, label, ports and error.
fn paint(painter: &Painter, l: &Layout, label: &str, selected: bool) {
    let (font, small) = (FontId::proportional(13.0), FontId::proportional(11.0));
    let fill = if selected { theme::LIGHTER } else { theme::DARKER };
    painter.rect_filled(l.rect, 0.0, fill);
    let title = l.rect.min + vec2(8.0, HEADER / 2.0);
    painter.text(title, Align2::LEFT_CENTER, label, font, theme::TEXT);
    let ports = l.inputs.iter().map(|p| (p, 8.0, Align2::LEFT_CENTER));
    let ports = ports.chain(l.outputs.iter().map(|p| (p, -8.0, Align2::RIGHT_CENTER)));
    for ((name, pos), offset, align) in ports {
        painter.circle_filled(*pos, PORT, theme::TEXT);
        painter.text(*pos + vec2(offset, 0.0), align, name, small.clone(), theme::WEAK);
    }
    if let Some((at, error)) = &l.error {
        painter.text(*at, Align2::LEFT_CENTER, elide(error, 26), small, theme::ERROR);
    }
}

/// From the top: header, one row per port pair, an error row if the node
/// failed, the body its kind's GUI draws.
fn layout(id: NodeId, node: &Node, result: Option<&Presentation>) -> Layout {
    let error = match result {
        Some(Err(NodeError::Upstream(_))) | Some(Ok(_)) | None => None,
        Some(Err(e)) => Some(e.to_string()),
    };
    let size = gui::of(node.kind).size(node);
    let body_top = ports(node) + if error.is_some() { ROW } else { 0.0 };
    let pos = position(&node.ui, id).to_pos2();
    let port = |i: usize, x: f32| pos + vec2(x, HEADER + ROW * (i as f32 + 0.5));
    Layout {
        id,
        rect: Rect::from_min_size(pos, vec2(size.x, body_top + size.y)),
        inputs: node.kind.inputs.iter().enumerate().map(|(i, p)| (p.name, port(i, 0.0))).collect(),
        outputs: node
            .kind
            .outputs
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name, port(i, size.x)))
            .collect(),
        error: error.map(|e| (pos + vec2(8.0, ports(node) + ROW / 2.0), e)),
        body: Rect::from_min_size(pos + vec2(0.0, body_top), size),
    }
}

/// The height of a node's header and port rows.
fn ports(node: &Node) -> f32 {
    HEADER + ROW * node.kind.inputs.len().max(node.kind.outputs.len()) as f32
}

/// A node's saved position, or a spot derived from its id for nodes never placed.
fn position(ui: &Json, id: NodeId) -> Vec2 {
    pair(&ui["pos"])
        .unwrap_or_else(|| vec2(40.0 + 200.0 * (id.0 % 5) as f32, 40.0 + 140.0 * (id.0 / 5) as f32))
}

/// A node's rectangle in graph units, without an error row.
fn bounds(id: NodeId, node: &Node) -> Rect {
    let size = gui::of(node.kind).size(node);
    Rect::from_min_size(position(&node.ui, id).to_pos2(), vec2(size.x, ports(node) + size.y))
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
