use super::*;
use egui::{
    Event, Modifiers,
    PointerButton::{Primary, Secondary},
};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::json;

struct Scene {
    editor: Editor,
    graph: Graph,
    node: NodeId,
    selected: Option<NodeId>,
    area: Rect,
    edited: bool,
    refused: Option<String>,
}

impl Scene {
    fn node_point(&self) -> Pos2 {
        let node = self.graph.node(self.node).unwrap();
        self.area.min
            + self.editor.offset.unwrap()
            + (position(&node.ui, self.node) + vec2(40.0, 10.0)) * self.editor.zoom
    }

    fn port_point(&self, id: NodeId, direction: Direction) -> Pos2 {
        let layout = layout(id, self.graph.node(id).unwrap(), None);
        let ports = match direction {
            Direction::Input => layout.inputs,
            Direction::Output => layout.outputs,
        };
        self.area.min + self.editor.offset.unwrap() + ports[0].1.to_vec2() * self.editor.zoom
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
        area: Rect::NOTHING,
        edited: false,
        refused: None,
    };
    Harness::builder()
        .with_size(vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(30)
        .build_ui_state(
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
                    s.area = ui.available_rect_before_wrap();
                    s.editor.show(
                        ui,
                        &mut s.graph,
                        &drip::nodes::registry(),
                        &mut s.selected,
                        &mut frame,
                    );
                });
                s.edited |= frame.report.edited;
                if let Some(refused) = frame.report.refused {
                    s.refused = Some(refused);
                }
            },
            scene,
        )
}

fn pointer(h: &mut Harness<'_, Scene>, pos: Pos2, button: PointerButton, pressed: bool) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
    h.step();
    if !pressed {
        h.run();
    }
}

fn drag(h: &mut Harness<'_, Scene>, pos: Pos2, button: PointerButton, delta: Vec2) {
    pointer(h, pos, button, true);
    h.event(Event::PointerMoved(pos + delta));
    h.step();
    pointer(h, pos + delta, button, false);
}

fn click(h: &mut Harness<'_, Scene>, pos: Pos2, button: PointerButton) {
    pointer(h, pos, button, true);
    pointer(h, pos, button, false);
}

fn click_port(h: &mut Harness<'_, Scene>, id: NodeId, direction: Direction, button: PointerButton) {
    let pos = h.state().port_point(id, direction);
    click(h, pos, button);
}

fn add_node(h: &mut Harness<'_, Scene>, kind: &'static drip::node::NodeKind, pos: Vec2) -> NodeId {
    let graph = &mut h.state_mut().graph;
    let id = graph.add_node(kind);
    graph.set_ui(id, json!({"pos": [pos.x, pos.y]})).unwrap();
    h.run();
    id
}

fn image_port(id: NodeId) -> Port {
    Port(id, "image".into())
}

#[test]
fn ports_connect_in_either_direction_and_preserve_sources_until_commit() {
    for zoom in [0.5, 1.0, 2.0] {
        for start_at_input in [false, true] {
            let mut h = harness(zoom);
            let source = h.state().node;
            let target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(240.0, 180.0));
            let previous = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(100.0, 240.0));
            h.state_mut().graph.connect(image_port(previous), image_port(target)).unwrap();
            let before = h.state().graph.clone();
            let (start, end) = if start_at_input {
                ((target, Direction::Input), (source, Direction::Output))
            } else {
                ((source, Direction::Output), (target, Direction::Input))
            };
            click_port(&mut h, start.0, start.1, Secondary);
            assert!(h.state().editor.wire.is_some());
            assert_eq!(h.state().graph, before);
            assert!(!h.state().edited);
            click_port(&mut h, end.0, end.1, Secondary);
            assert!(h.state().editor.wire.is_none());
            assert_eq!(h.state().graph.source(&image_port(target)), Some(&image_port(source)));
            assert!(h.state().edited);
            assert_eq!(h.state().selected, None);
            assert!(h.query_by_label("Rename").is_none());

            click_port(&mut h, source, Direction::Output, Secondary);
            click_port(&mut h, previous, Direction::Input, Secondary);
            assert_eq!(h.state().graph.source(&image_port(previous)), Some(&image_port(source)));
            assert_eq!(h.state().graph.edges().count(), 2);
        }
    }
}

#[test]
fn invalid_port_targets_keep_the_pending_connection_and_existing_source() {
    let mut h = harness(1.0);
    let source = h.state().node;
    let target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(400.0, 100.0));
    let camera = add_node(&mut h, &drip::nodes::RCD, vec2(100.0, 300.0));
    h.state_mut().graph.connect(image_port(source), image_port(target)).unwrap();
    let before = h.state().graph.clone();
    click_port(&mut h, target, Direction::Input, Secondary);
    let pending = h.state().editor.wire.clone();
    for (id, direction, error) in [
        (source, Direction::Input, "Choose an output port"),
        (camera, Direction::Output, "Exposure 2 · image requires Color image, got Camera RGB"),
        (target, Direction::Output, "cycle"),
    ] {
        click_port(&mut h, id, direction, Secondary);
        assert!(h.state().refused.as_ref().unwrap().contains(error));
        assert_eq!(h.state().editor.wire, pending);
        assert_eq!(h.state().graph, before);
        assert!(!h.state().edited);
    }
    click_port(&mut h, source, Direction::Output, Secondary);
    assert!(h.state().editor.wire.is_none());
}

#[test]
fn pending_connections_cancel_without_opening_the_canvas_menu() {
    for direction in [Direction::Input, Direction::Output] {
        let mut h = harness(1.0);
        let source = h.state().node;
        let target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(400.0, 100.0));
        h.state_mut().graph.connect(image_port(source), image_port(target)).unwrap();
        let before = h.state().graph.clone();
        let start = if direction == Direction::Input { target } else { source };
        click_port(&mut h, start, direction, Secondary);
        click(&mut h, pos2(40.0, 40.0), Secondary);
        assert!(h.state().editor.wire.is_none());
        assert!(h.query_by_label("color ⏵").is_none());
        click_port(&mut h, start, direction, Secondary);
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.state().editor.wire.is_none());
        assert_eq!(h.state().graph, before);
        assert!(!h.state().edited);
    }
}

#[test]
fn port_drags_pan_only_with_the_left_button() {
    for direction in [Direction::Input, Direction::Output] {
        for zoom in [0.5, 1.0, 2.0] {
            let mut h = harness(zoom);
            let id = h.state().node;
            let before = h.state().graph.clone();
            let offset = h.state().editor.offset.unwrap();
            let pos = h.state().port_point(id, direction);
            let delta = vec2(50.0, 30.0);
            drag(&mut h, pos, Primary, delta);
            assert_eq!(h.state().editor.offset.unwrap(), offset + delta);
            let pos = h.state().port_point(id, direction);
            drag(&mut h, pos, Secondary, delta);
            assert_eq!(h.state().editor.offset.unwrap(), offset + delta);
            assert_eq!(h.state().graph, before);
            assert_eq!(h.state().selected, None);
            assert!(h.state().editor.wire.is_none());
            assert!(h.query_by_label("No source").is_none());
            assert!(h.query_by_label("No destinations").is_none());
        }
    }
}

#[test]
fn port_menus_navigate_to_connected_nodes_without_editing() {
    let mut h = harness(1.0);
    let source = h.state().node;
    let near = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(400.0, 100.0));
    let far = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(3000.0, 1000.0));
    for id in [near, far] {
        h.state_mut().graph.connect(image_port(source), image_port(id)).unwrap();
    }
    let before = h.state().graph.clone();
    let offset = h.state().editor.offset;
    click_port(&mut h, source, Direction::Output, Primary);
    assert_eq!(h.state().selected, None);
    assert!(h.query_by_label("Exposure 3 · image").is_some());
    h.get_by_label("Exposure 2 · image").click();
    h.run();
    assert_eq!(h.state().selected, Some(near));
    assert_eq!(h.state().editor.offset, offset);

    click_port(&mut h, source, Direction::Output, Primary);
    h.get_by_label("Exposure 3 · image").click();
    h.run();
    assert_eq!(h.state().selected, Some(far));
    let visible = h.state().area;
    assert!(visible.contains(h.state().port_point(far, Direction::Input)));
    assert!(visible.contains(h.state().port_point(far, Direction::Output)));
    assert_eq!(h.state().editor.zoom, 1.0);

    click_port(&mut h, far, Direction::Input, Primary);
    assert_eq!(h.state().selected, Some(far));
    h.get_by_label("Exposure · image").click();
    h.run();
    assert_eq!(h.state().selected, Some(source));
    assert!(visible.contains(h.state().node_point()));
    assert_eq!(h.state().graph, before);
    assert!(!h.state().edited);
}

#[test]
fn unconnected_ports_show_empty_navigation_menus() {
    for (direction, label) in
        [(Direction::Input, "No source"), (Direction::Output, "No destinations")]
    {
        let mut h = harness(1.0);
        let id = h.state().node;
        click_port(&mut h, id, direction, Primary);
        assert!(h.get_by_label(label).accesskit_node().is_disabled());
        assert_eq!(h.state().selected, None);
        assert!(!h.state().edited);
    }
}

#[test]
fn wire_hit_band_stays_in_screen_points_and_removes_only_its_connection() {
    for zoom in [0.5, 1.0, 2.0] {
        for distance in [0.0, 4.0, 8.0] {
            let mut h = harness(zoom);
            let source = h.state().node;
            let target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(340.0, 118.0));
            let branch = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(340.0, 230.0));
            for id in [target, branch] {
                h.state_mut().graph.connect(image_port(source), image_port(id)).unwrap();
            }
            let start = h.state().port_point(source, Direction::Output);
            let end = h.state().port_point(target, Direction::Input);
            let pointer = start.lerp(end, 0.5) + vec2(0.0, distance);
            h.event(Event::PointerMoved(pointer));
            h.run();
            assert_eq!(
                h.output().platform_output.cursor_icon == egui::CursorIcon::PointingHand,
                distance <= WIRE_HIT
            );
            click(&mut h, pointer, Secondary);
            let removed = distance <= WIRE_HIT;
            assert_eq!(h.state().graph.source(&image_port(target)).is_none(), removed);
            assert_eq!(h.state().graph.source(&image_port(branch)), Some(&image_port(source)));
            assert_eq!(h.state().edited, removed);
            assert_eq!(h.query_by_label("color ⏵").is_some(), !removed);
        }
    }
}

#[test]
fn crossing_wires_choose_the_nearest_curve() {
    let mut h = harness(1.0);
    let upper = h.state().node;
    let lower = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(100.0, 202.0));
    let upper_target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(440.0, 118.0));
    let lower_target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(440.0, 220.0));
    h.state_mut().graph.connect(image_port(upper), image_port(lower_target)).unwrap();
    h.state_mut().graph.connect(image_port(lower), image_port(upper_target)).unwrap();
    let curve = bezier(
        h.state().port_point(upper, Direction::Output),
        h.state().port_point(lower_target, Direction::Input),
        Stroke::NONE,
    );
    let pointer = curve.sample(0.48);
    click(&mut h, pointer, Secondary);
    assert!(h.state().graph.source(&image_port(lower_target)).is_none());
    assert_eq!(h.state().graph.source(&image_port(upper_target)), Some(&image_port(lower)));
}

#[test]
fn wires_leave_panning_and_overlapping_nodes_and_ports_interactive() {
    for over_port in [false, true] {
        let mut h = harness(1.0);
        let source = h.state().node;
        let target = add_node(&mut h, &drip::nodes::EXPOSURE, vec2(440.0, 118.0));
        h.state_mut().graph.connect(image_port(source), image_port(target)).unwrap();
        let start = h.state().port_point(source, Direction::Output);
        let end = h.state().port_point(target, Direction::Input);
        let midpoint = start.lerp(end, 0.5);
        let offset = h.state().editor.offset.unwrap();
        let delta = vec2(30.0, 20.0);
        drag(&mut h, midpoint, Primary, delta);
        assert_eq!(h.state().editor.offset.unwrap(), offset + delta);
        assert_eq!(h.state().graph.source(&image_port(target)), Some(&image_port(source)));
        let position = if over_port { vec2(350.0, 118.0) } else { vec2(330.0, 139.0) };
        let overlay = add_node(&mut h, &drip::nodes::EXPOSURE, position);
        click(&mut h, midpoint + delta, Secondary);
        assert_eq!(h.state().graph.source(&image_port(target)), Some(&image_port(source)));
        assert!(!h.state().edited);
        if over_port {
            assert_eq!(
                h.state().editor.wire,
                Some(Endpoint { port: image_port(overlay), direction: Direction::Input })
            );
            assert!(h.query_by_label("Rename").is_none());
        } else {
            assert!(h.query_by_label("Rename").is_some());
            assert!(h.state().editor.wire.is_none());
        }
    }
}

#[test]
fn deleting_the_start_node_cancels_a_pending_connection() {
    let mut h = harness(1.0);
    let id = h.state().node;
    click_port(&mut h, id, Direction::Output, Secondary);
    let pos = h.state().node_point();
    click(&mut h, pos, Primary);
    h.key_press(egui::Key::Delete);
    h.run();
    assert!(h.state().graph.node(id).is_none());
    assert!(h.state().editor.wire.is_none());
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
        assert!(h.query_by_label("color ⏵").is_none());
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
    assert_eq!(h.state().graph.node(node).unwrap().name, "renamed");
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
    let categories = ["color", "demosaic", "export", "raw", "tone", "view"];
    let rows: Vec<_> =
        categories.iter().map(|name| h.get_by_label(&format!("{name} ⏵")).rect().top()).collect();
    assert!(rows.windows(2).all(|rows| rows[0] < rows[1]));
    h.get_by_label("color ⏵").click();
    h.run();
    let names = ["Camera to Rec.2020", "Exposure", "White balance"];
    let rows: Vec<_> = names.iter().map(|name| h.get_by_label(name).rect().top()).collect();
    assert!(rows.windows(2).all(|rows| rows[0] < rows[1]));
    h.get_by_label(drip::nodes::EXPOSURE.name).click();
    h.run();
    assert_eq!(h.state().graph.nodes().count(), 1);
    let added = h.state().graph.node(h.state().selected.unwrap()).unwrap();
    assert_eq!(added.kind.id, "color.exposure");
    assert_eq!(added.name, "Exposure");
    assert!(!egui::Popup::is_any_open(&h.ctx));
}
