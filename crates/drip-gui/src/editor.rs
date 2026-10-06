//! The node editor: the main canvas. It draws straight from
//! the graph and edits it only through the graph's validated operations. Node
//! positions and sizes live in each node's opaque `ui` field, in graph units.
//! The canvas is an egui layer that egui transforms to pan and zoom, so
//! everything on it is drawn and interacted with in graph units. The editor
//! draws each node's frame (header, ports, error) and its kind's GUI the body.

use crate::editing::{Edit, Frame, NodeCx, pair};
use crate::node_ui::{self, Part};
use crate::widgets::{self, BUTTON};
use crate::worker::Presentation;
use drip::eval::NodeError;
use drip::graph::{Graph, Node, NodeId, Port};
use drip::node::Registry;
use egui::emath::TSTransform;
use egui::epaint::CubicBezierShape;
use egui::{
    Align2, FontId, LayerId, Painter, PointerButton, Pos2, Rect, Sense, Stroke, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use serde_json::Value as Json;

use crate::theme;

mod node_menu;

const HEADER: f32 = 22.0;
const ROW: f32 = 18.0;
const PORT: f32 = 4.0;
/// Wire hit radius in screen points, independent of canvas zoom.
const WIRE_HIT: f32 = 6.0;
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
    /// The port awaiting an opposite-direction endpoint on the next right-click.
    wire: Option<Endpoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Direction {
    Input,
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    port: Port,
    direction: Direction,
}

impl Default for Editor {
    fn default() -> Self {
        Editor { offset: None, zoom: 1.0, wire: None }
    }
}

/// Where a node and its parts are in graph coordinates.
struct NodeLayout {
    id: NodeId,
    rect: Rect,
    inputs: Vec<(&'static str, Pos2)>,
    outputs: Vec<(&'static str, Pos2)>,
    error: Option<(Pos2, String)>,
    body: Rect,
}

/// Geometry snapshot shared by drawing and interaction for one frame.
/// Graph edits take effect in the next frame's layout.
struct CanvasLayout {
    nodes: Vec<NodeLayout>,
}

impl CanvasLayout {
    fn new<'a>(graph: &Graph, results: &dyn Fn(NodeId) -> Option<&'a Presentation>) -> Self {
        Self { nodes: graph.nodes().map(|(id, node)| layout(id, node, results(id))).collect() }
    }

    fn port_position(&self, port: &Port, direction: Direction) -> Option<Pos2> {
        let node = self.nodes.iter().find(|l| l.id == port.0)?;
        let ports = match direction {
            Direction::Input => &node.inputs,
            Direction::Output => &node.outputs,
        };
        ports.iter().find(|(name, _)| *name == port.1).map(|(_, pos)| *pos)
    }

    fn node_rect(&self, id: NodeId) -> Option<Rect> {
        self.nodes.iter().find(|l| l.id == id).map(|l| l.rect)
    }
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
        if self.wire.is_some()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.wire = None;
        }
        let (mut canvas, to_global) = self.canvas(ui, graph, selected);
        let background = canvas.response();
        let zoom = to_global.scaling;
        let ui = &mut canvas;
        let painter = ui.painter().clone();

        let layout = CanvasLayout::new(graph, frame.results);

        // Wires thin out slower than the zoom; port hit areas keep their
        // on-screen size.
        let point = widgets::point(ui);
        let wire = Stroke::new(1.5 * zoom.sqrt() * point, theme::TEXT);
        // Hit-test only the background: nodes, ports and controls retain priority.
        let pointer = background.hover_pos().filter(|_| self.wire.is_none());
        let disconnect = draw_wires(ui, graph, &layout, pointer, wire);

        let mut navigate = None;
        for node in &layout.nodes {
            if let Some(id) = self.show_node(ui, node, graph, selected, frame) {
                navigate = Some(id);
            }
        }

        if let Some(id) = navigate {
            *selected = Some(id);
            let rect = layout.node_rect(id).expect("connected node");
            let visible = ui.clip_rect();
            if !visible.contains_rect(rect) {
                *self.offset.as_mut().expect("canvas initialized") +=
                    (visible.center() - rect.center()) * zoom;
            }
            ui.ctx().request_repaint();
        }

        if background.secondary_clicked() && self.wire.is_some() {
            self.wire = None;
        } else if background.secondary_clicked()
            && let Some(input) = disconnect
        {
            frame.edit(graph, Edit::Disconnect(input));
        } else {
            theme::context_menu(&background).show(|ui| {
                if let Some(kind) = node_menu::show(ui, registry) {
                    let pointer = ui
                        .ctx()
                        .pointer_interact_pos()
                        .unwrap_or(to_global * background.rect.center());
                    let pos = (to_global.inverse() * pointer).to_vec2();
                    *selected = frame.edit(graph, Edit::Add(kind, pos));
                }
            });
        }

        if let Some(id) = *selected
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            frame.edit(graph, Edit::Remove(id));
            *selected = None;
        }

        if self.wire.as_ref().is_some_and(|from| graph.node(from.port.0).is_none()) {
            self.wire = None;
        }
        if let Some(from) = &self.wire {
            let pointer = ui.ctx().pointer_latest_pos().map(|p| to_global.inverse() * p);
            let output = from.direction == Direction::Output;
            if let (Some(start), Some(end)) =
                (layout.port_position(&from.port, from.direction), pointer)
            {
                let (start, end) = if output { (start, end) } else { (end, start) };
                painter.add(bezier(start, end, wire));
            }
        }
    }

    fn show_node(
        &mut self,
        ui: &mut Ui,
        l: &NodeLayout,
        graph: &mut Graph,
        selected: &mut Option<NodeId>,
        frame: &mut Frame,
    ) -> Option<NodeId> {
        let painter = ui.painter().clone();
        let point = widgets::point(ui);
        let hit = |pos: Pos2| Rect::from_center_size(pos, Vec2::splat(16.0 * point));
        let mut navigate = None;
        let body = ui.interact(l.rect, ui.id().with(l.id), Sense::click_and_drag());
        if body.clicked() || body.drag_started_by(PointerButton::Secondary) {
            *selected = Some(l.id);
        }
        self.pan(&body);
        if body.dragged_by(PointerButton::Secondary) {
            let pos = position(&graph.node(l.id).expect("laid out").ui, l.id) + body.drag_delta();
            frame.edit(graph, Edit::Ui(l.id, "pos", pos));
        }
        let mut remove = false;
        theme::context_menu(&body).show(|ui| {
            if ui.button("Rename").clicked() {
                *selected = Some(l.id);
                node_ui::parameters::focus_name(ui.ctx(), l.id);
                ui.close();
            }
            if ui.button("Delete").clicked() {
                remove = true;
                ui.close();
            }
        });
        if remove {
            frame.edit(graph, Edit::Remove(l.id));
            if *selected == Some(l.id) {
                *selected = None;
            }
            return None;
        }
        let node = graph.node(l.id).expect("laid out from the graph");
        let descriptor = node.kind;
        let (kind, kind_params) = (node_ui::of(node.kind), node.kind.params);
        paint(&painter, l, node, *selected == Some(l.id));
        let inputs = l
            .inputs
            .iter()
            .zip(descriptor.inputs())
            .map(|((name, pos), spec)| (Direction::Input, *name, *pos, spec.requirement.name));
        let outputs = l
            .outputs
            .iter()
            .zip(descriptor.outputs())
            .map(|((name, pos), spec)| (Direction::Output, *name, *pos, spec.ty.name));
        for (direction, name, pos, contract) in inputs.chain(outputs) {
            let endpoint = Endpoint { port: Port(l.id, name.into()), direction };
            let response = ui
                .interact(hit(pos), ui.id().with((l.id, name, direction)), Sense::click_and_drag())
                .on_hover_text(node_ui::ports::label(contract));
            self.pan(&response);
            if response.secondary_clicked() {
                egui::Popup::close_all(ui.ctx());
                self.connect(endpoint.clone(), graph, frame);
            }
            theme::menu(&response).show(|ui| {
                if let Some(id) = peer_menu(ui, graph, &endpoint) {
                    navigate = Some(id);
                }
            });
        }
        if let Some((at, error)) = &l.error {
            let row = Rect::from_min_size(
                pos2(l.rect.min.x, at.y - ROW / 2.0),
                vec2(l.rect.width(), ROW),
            );
            ui.interact(row, ui.id().with((l.id, "error")), Sense::hover()).on_hover_text(error);
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
        if body.hovered() {
            painter.rect_stroke(
                l.rect,
                0.0,
                Stroke::new(point, theme::TEXT),
                egui::StrokeKind::Inside,
            );
        }
        navigate
    }

    fn pan(&mut self, response: &egui::Response) {
        let delta = response.ctx.input(|i| i.pointer.delta());
        if response.dragged_by(PointerButton::Primary) && delta != Vec2::ZERO {
            *self.offset.as_mut().expect("canvas initialized") += delta;
            response.ctx.request_repaint();
        }
    }

    fn connect(&mut self, target: Endpoint, graph: &mut Graph, frame: &mut Frame) {
        let Some(from) = &self.wire else {
            self.wire = Some(target);
            return;
        };
        if from.direction == target.direction {
            frame.report.refused = Some(match from.direction {
                Direction::Input => "Choose an output port".into(),
                Direction::Output => "Choose an input port".into(),
            });
            return;
        }
        let (output, input) = match from.direction {
            Direction::Output => (&from.port, &target.port),
            Direction::Input => (&target.port, &from.port),
        };
        frame.edit(graph, Edit::Connect(output.clone(), input.clone()));
        if graph.source(input) == Some(output) {
            self.wire = None;
        }
    }

    /// Builds the canvas layer and handles background panning, zoom and selection.
    fn canvas(
        &mut self,
        ui: &mut Ui,
        graph: &Graph,
        selected: &mut Option<NodeId>,
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
        if background.dragged_by(PointerButton::Primary) {
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
        ui.ctx().set_transform_layer(layer, to_global);
        let visible = to_global.inverse() * area;
        canvas.set_clip_rect(visible);
        canvas.expand_to_include_rect(visible);
        (canvas, to_global)
    }
}

fn draw_wires(
    ui: &mut Ui,
    graph: &Graph,
    layout: &CanvasLayout,
    pointer: Option<Pos2>,
    wire: Stroke,
) -> Option<Port> {
    let point = widgets::point(ui);
    let wires: Vec<_> = graph
        .edges()
        .filter_map(|(output, input)| {
            Some((
                input.clone(),
                bezier(
                    layout.port_position(output, Direction::Output)?,
                    layout.port_position(input, Direction::Input)?,
                    wire,
                ),
            ))
        })
        .collect();
    let hovered_wire = pointer.and_then(|pointer| {
        wires
            .iter()
            .enumerate()
            .map(|(i, (_, curve))| (i, wire_distance(curve, pointer, point)))
            .filter(|(_, distance)| *distance <= WIRE_HIT * point)
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, _)| i)
    });
    let disconnect = hovered_wire.map(|i| wires[i].0.clone());
    for (i, (_, mut curve)) in wires.into_iter().enumerate() {
        if hovered_wire == Some(i) {
            curve.stroke.width += 2.0 * point;
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().add(curve);
    }
    disconnect
}

fn peer_menu(ui: &mut Ui, graph: &Graph, endpoint: &Endpoint) -> Option<NodeId> {
    let mut navigate = None;
    let peers = graph.edges().filter_map(|(output, input)| match endpoint.direction {
        Direction::Input => (input == &endpoint.port).then_some(output),
        Direction::Output => (output == &endpoint.port).then_some(input),
    });
    let mut empty = true;
    for peer in peers {
        empty = false;
        let node = graph.node(peer.0).expect("connected node");
        if ui.button(format!("{} · {}", node.name, peer.1)).clicked() {
            navigate = Some(peer.0);
            ui.close();
        }
    }
    if empty {
        let text = match endpoint.direction {
            Direction::Input => "No source",
            Direction::Output => "No destinations",
        };
        ui.add_enabled(false, egui::Button::new(text));
    }
    navigate
}

/// Paints a node's frame: its fill, label, ports and error.
fn paint(painter: &Painter, l: &NodeLayout, node: &Node, selected: bool) {
    let (font, small) =
        (FontId::proportional(theme::BODY_SIZE), FontId::proportional(theme::SMALL_SIZE));
    let fill = if selected { theme::LIGHTER } else { theme::DARKER };
    painter.rect_filled(l.rect, 0.0, fill);
    let title = l.rect.min + vec2(8.0, HEADER / 2.0);
    painter.text(title, Align2::LEFT_CENTER, &node.name, font, theme::TEXT);
    let inputs = l.inputs.iter().zip(node.kind.inputs()).map(|(p, spec)| {
        (p, node_ui::ports::label(spec.requirement.name), 8.0, Align2::LEFT_CENTER)
    });
    let outputs = l
        .outputs
        .iter()
        .zip(node.kind.outputs())
        .map(|(p, spec)| (p, node_ui::ports::label(spec.ty.name), -8.0, Align2::RIGHT_CENTER));
    for ((_, pos), label, offset, align) in inputs.chain(outputs) {
        painter.circle_filled(*pos, PORT, theme::TEXT);
        let mut job = egui::text::LayoutJob::simple(
            label.into(),
            small.clone(),
            theme::WEAK,
            l.rect.width() - 16.0,
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = painter.layout_job(job);
        let rect = align.anchor_size(*pos + vec2(offset, 0.0), galley.size());
        painter.galley(rect.min, galley, theme::WEAK);
    }
    if let Some((at, error)) = &l.error {
        painter.text(*at, Align2::LEFT_CENTER, elide(error, 26), small, theme::ERROR);
    }
}

/// From the top: header, input rows, output rows, an error row if the node
/// failed, the body its kind's GUI draws.
fn layout(id: NodeId, node: &Node, result: Option<&Presentation>) -> NodeLayout {
    let error = match result {
        Some(Err(NodeError::Upstream(_))) | Some(Ok(_)) | None => None,
        Some(Err(e)) => Some(e.to_string()),
    };
    let size = node_ui::of(node.kind).size(node);
    let body_top = ports(node) + if error.is_some() { ROW } else { 0.0 };
    let pos = position(&node.ui, id).to_pos2();
    let port = |i: usize, x: f32| pos + vec2(x, HEADER + ROW * (i as f32 + 0.5));
    NodeLayout {
        id,
        rect: Rect::from_min_size(pos, vec2(size.x, body_top + size.y)),
        inputs: node.kind.inputs().enumerate().map(|(i, p)| (p.name, port(i, 0.0))).collect(),
        outputs: node
            .kind
            .outputs()
            .enumerate()
            .map(|(i, p)| (p.name, port(node.kind.inputs().len() + i, size.x)))
            .collect(),
        error: error.map(|e| (pos + vec2(8.0, ports(node) + ROW / 2.0), e)),
        body: Rect::from_min_size(pos + vec2(0.0, body_top), size),
    }
}

/// The height of a node's header and port rows.
fn ports(node: &Node) -> f32 {
    HEADER + ROW * (node.kind.inputs().len() + node.kind.outputs().len()) as f32
}

/// A node's saved position, or a spot derived from its id for nodes never placed.
fn position(ui: &Json, id: NodeId) -> Vec2 {
    pair(&ui["pos"])
        .unwrap_or_else(|| vec2(40.0 + 200.0 * (id.0 % 5) as f32, 40.0 + 140.0 * (id.0 / 5) as f32))
}

/// A node's rectangle in graph units, without an error row.
fn bounds(id: NodeId, node: &Node) -> Rect {
    let size = node_ui::of(node.kind).size(node);
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

fn wire_distance(curve: &CubicBezierShape, pointer: Pos2, point: f32) -> f32 {
    if !curve.logical_bounding_rect().expand(WIRE_HIT * point).contains(pointer) {
        return f32::INFINITY;
    }
    curve
        .flatten(Some(0.25 * point))
        .windows(2)
        .map(|segment| {
            let delta = segment[1] - segment[0];
            let direction = delta.normalized();
            let along = (pointer - segment[0]).dot(direction).clamp(0.0, delta.length());
            pointer.distance(segment[0] + direction * along)
        })
        .fold(f32::INFINITY, f32::min)
}

fn elide(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        text.into()
    } else {
        text.chars().take(chars - 1).chain(['…']).collect()
    }
}

#[cfg(test)]
mod tests;
