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
            if body.clicked() || body.drag_started_by(PointerButton::Secondary) {
                *selected = Some(l.id);
            }
            if body.dragged_by(PointerButton::Primary)
                && ui.input(|i| i.pointer.delta()) != Vec2::ZERO
            {
                *self.offset.as_mut().expect("canvas initialized") +=
                    ui.input(|i| i.pointer.delta());
                ui.ctx().request_repaint();
            }
            if body.dragged_by(PointerButton::Secondary) {
                let pos =
                    position(&graph.node(l.id).expect("laid out").ui, l.id) + body.drag_delta();
                frame.edit(graph, Edit::Ui(l.id, "pos", pos));
            }
            let mut remove = false;
            theme::context_menu(&body).show(|ui| {
                if ui.button("Rename").clicked() {
                    *selected = Some(l.id);
                    node_ui::parameters::focus_label(ui.ctx(), l.id);
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
                continue;
            }
            let node = graph.node(l.id).expect("laid out from the graph");
            let descriptor = node.kind;
            let (kind, kind_params) = (node_ui::of(node.kind), node.kind.params);
            paint(&painter, l, node, *selected == Some(l.id));
            for ((name, pos), spec) in l.inputs.iter().zip(descriptor.inputs()) {
                let port = ui.interact(hit(*pos), ui.id().with((l.id, name, 0)), Sense::click());
                if port.secondary_clicked() {
                    frame.edit(graph, Edit::Disconnect(Port(l.id, (*name).into())));
                }
                port.on_hover_text(format!(
                    "{}\nright-click to disconnect",
                    node_ui::ports::label(spec.requirement.name)
                ));
            }
            for ((name, pos), spec) in l.outputs.iter().zip(descriptor.outputs()) {
                if ui
                    .interact(hit(*pos), ui.id().with((l.id, name, 1)), Sense::drag())
                    .on_hover_text(node_ui::ports::label(spec.ty.name))
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
                    frame.edit(graph, Edit::Connect(from, Port(id, name.into())));
                }
            }
        }

        if let Some(id) = *selected
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
        {
            frame.edit(graph, Edit::Remove(id));
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
        theme::context_menu(&background).show(|ui| {
            for kind in registry.kinds() {
                if ui.button(kind.label).clicked() {
                    let pointer = ui.ctx().pointer_interact_pos().unwrap_or(area.center());
                    let pos = (to_global.inverse() * pointer).to_vec2();
                    *selected = frame.edit(graph, Edit::Add(kind, pos));
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
fn paint(painter: &Painter, l: &Layout, node: &Node, selected: bool) {
    let (font, small) =
        (FontId::proportional(theme::BODY_SIZE), FontId::proportional(theme::SMALL_SIZE));
    let fill = if selected { theme::LIGHTER } else { theme::DARKER };
    painter.rect_filled(l.rect, 0.0, fill);
    let title = l.rect.min + vec2(8.0, HEADER / 2.0);
    painter.text(title, Align2::LEFT_CENTER, &node.label, font, theme::TEXT);
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
fn layout(id: NodeId, node: &Node, result: Option<&Presentation>) -> Layout {
    let error = match result {
        Some(Err(NodeError::Upstream(_))) | Some(Ok(_)) | None => None,
        Some(Err(e)) => Some(e.to_string()),
    };
    let size = node_ui::of(node.kind).size(node);
    let body_top = ports(node) + if error.is_some() { ROW } else { 0.0 };
    let pos = position(&node.ui, id).to_pos2();
    let port = |i: usize, x: f32| pos + vec2(x, HEADER + ROW * (i as f32 + 0.5));
    Layout {
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

fn elide(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        text.into()
    } else {
        text.chars().take(chars - 1).chain(['…']).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{
        Event, Modifiers,
        PointerButton::{Primary, Secondary},
    };
    use egui_kittest::{Harness, kittest::Queryable};
    use serde_json::json;

    struct Scene {
        editor: Editor,
        graph: Graph,
        node: NodeId,
        selected: Option<NodeId>,
        origin: Pos2,
        edited: bool,
    }

    impl Scene {
        fn node_point(&self) -> Pos2 {
            let node = self.graph.node(self.node).unwrap();
            self.origin
                + self.editor.offset.unwrap()
                + (position(&node.ui, self.node) + vec2(40.0, 10.0)) * self.editor.zoom
        }
    }

    fn harness(zoom: f32) -> Harness<'static, Scene> {
        let mut graph = Graph::default();
        let node = graph.add_node(&drip::nodes::EXPOSURE);
        graph.set_ui(node, json!({"pos": [100.0, 100.0]})).unwrap();
        let scene = Scene {
            editor: Editor { offset: Some(vec2(20.0, 20.0)), zoom, wire: None },
            graph,
            node,
            selected: None,
            origin: Pos2::ZERO,
            edited: false,
        };
        Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, s: &mut Scene| {
                theme::apply(ui.ctx());
                let popped = Default::default();
                let mut frame = Frame::new(&|_| None, &popped, false);
                egui::Panel::right("inspector").default_size(250.0).show(ui, |ui| {
                    if let Some(id) = s.selected {
                        crate::inspector::selected_node(
                            ui,
                            &mut NodeCx::new(&mut s.graph, id, &mut frame),
                        );
                    }
                });
                egui::CentralPanel::default().show(ui, |ui| {
                    s.origin = ui.available_rect_before_wrap().min;
                    s.editor.show(
                        ui,
                        &mut s.graph,
                        &drip::nodes::registry(),
                        &mut s.selected,
                        &mut frame,
                    );
                });
                s.edited |= frame.report.edited;
            },
            scene,
        )
    }

    fn pointer(h: &mut Harness<'_, Scene>, pos: Pos2, button: PointerButton, pressed: bool) {
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
        h.run();
    }

    fn drag(h: &mut Harness<'_, Scene>, pos: Pos2, button: PointerButton, delta: Vec2) {
        pointer(h, pos, button, true);
        h.event(Event::PointerMoved(pos + delta));
        h.run();
        pointer(h, pos + delta, button, false);
    }

    #[test]
    fn left_drag_pans_over_background_and_nodes_without_editing() {
        for zoom in [0.5, 1.0, 2.0] {
            for on_node in [false, true] {
                let mut h = harness(zoom);
                h.run();
                let graph = h.state().graph.clone();
                let offset = h.state().editor.offset.unwrap();
                let pos = if on_node { h.state().node_point() } else { pos2(40.0, 40.0) };
                let delta = vec2(50.0, 30.0);
                drag(&mut h, pos, Primary, delta);
                assert_eq!(h.state().editor.offset.unwrap(), offset + delta);
                assert_eq!(h.state().graph, graph);
                assert_eq!(h.state().selected, None);
                assert!(!h.state().edited);
            }
        }
    }

    #[test]
    fn right_drag_moves_only_nodes_without_opening_menus() {
        for zoom in [0.5, 1.0, 2.0] {
            let mut h = harness(zoom);
            h.run();
            let offset = h.state().editor.offset;
            let pos = h.state().node_point();
            let delta = vec2(50.0, 30.0);
            drag(&mut h, pos, Secondary, delta);
            let s = h.state();
            assert_eq!(
                position(&s.graph.node(s.node).unwrap().ui, s.node),
                vec2(100.0, 100.0) + delta / zoom
            );
            assert_eq!(s.editor.offset, offset);
            assert!(!s.edited);
            assert!(h.query_by_label("Rename").is_none());
            let graph = s.graph.clone();
            drag(&mut h, pos2(40.0, 40.0), Secondary, delta);
            assert_eq!(h.state().graph, graph);
            assert_eq!(h.state().editor.offset, offset);
            assert!(h.query_by_label(drip::nodes::EXPOSURE.label).is_none());
        }
    }

    #[test]
    fn click_selection_and_node_menu_actions() {
        let mut h = harness(1.0);
        h.run();
        let node = h.state().node;
        let pos = h.state().node_point();
        pointer(&mut h, pos, Primary, true);
        pointer(&mut h, pos, Primary, false);
        assert_eq!(h.state().selected, Some(node));
        assert!(h.query_by_label("ev").is_some());
        pointer(&mut h, pos2(40.0, 40.0), Primary, true);
        pointer(&mut h, pos2(40.0, 40.0), Primary, false);
        assert_eq!(h.state().selected, None);

        pointer(&mut h, pos, Secondary, true);
        pointer(&mut h, pos, Secondary, false);
        h.get_by_label("Rename").click();
        h.run();
        assert_eq!(h.state().selected, Some(node));
        h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
        h.event(Event::Text("renamed".into()));
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(h.state().graph.node(node).unwrap().label, "renamed");
        assert!(!h.state().edited);

        pointer(&mut h, pos, Secondary, true);
        pointer(&mut h, pos, Secondary, false);
        h.get_by_label("Delete").click();
        h.run();
        assert!(h.state().graph.node(node).is_none());
        assert_eq!(h.state().selected, None);
        assert!(h.state().edited);

        pointer(&mut h, pos2(40.0, 40.0), Secondary, true);
        pointer(&mut h, pos2(40.0, 40.0), Secondary, false);
        h.get_by_label(drip::nodes::EXPOSURE.label).click();
        h.run();
        assert_eq!(h.state().graph.nodes().count(), 1);
        assert!(h.state().selected.is_some());
    }
}
