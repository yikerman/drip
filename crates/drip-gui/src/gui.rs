//! Node GUIs (DESIGN G13). The editor draws every node's frame, with a button
//! popping out the node's parameters when it has any. Each node kind draws its
//! own body below that, in graph units, through a context limited to that
//! node; any part of the body can pop out into a window the kind draws.
//! Kinds without a GUI of their own get an empty body.

use std::collections::BTreeSet;

use drip::graph::{Graph, Node, NodeId};
use drip::node::NodeKind;
use drip::nodes::{HISTOGRAM, PREVIEW};
use egui::{FontId, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, vec2};
use serde_json::{Value as Json, json};

use crate::views::{self, PreparedView};
use crate::worker::Presentation;
use crate::{inspector, theme};

/// Width of a node whose body does not set one, in graph units.
pub const WIDTH: f32 = 160.0;
/// Space around a node's contents, in graph units.
pub const PAD: f32 = 6.0;
/// Side of a pop-out button, in graph units.
pub const BUTTON: f32 = 20.0;

pub trait NodeGui: Sync {
    /// The size of the node's body, below its ports, in graph units. Its
    /// width is the node's.
    fn size(&self, _node: &Node) -> Vec2 {
        vec2(WIDTH, PAD)
    }

    /// Draws the body into `ui`, whose max rect has the body's size.
    fn body(&self, _ui: &mut Ui, _node: &mut NodeCx) {}

    /// The initial size of the window the body popped out as `name`, in points.
    fn window_size(&self, _name: &'static str, _node: &Node) -> Vec2 {
        vec2(360.0, 320.0)
    }

    /// Draws the content of the window the body popped out as `name`.
    fn window(&self, _ui: &mut Ui, _name: &'static str, _node: &mut NodeCx) {}
}

/// The GUI of nodes of `kind`.
pub fn of(kind: &NodeKind) -> &'static dyn NodeGui {
    static GUIS: &[(&NodeKind, &dyn NodeGui)] = &[(&PREVIEW, &Viewer), (&HISTOGRAM, &Viewer)];
    GUIS.iter().find(|(k, _)| *k == kind).map_or(&Plain, |(_, gui)| *gui)
}

/// A popped-out window: one part of one node.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Popped {
    pub node: NodeId,
    pub part: Part,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Part {
    /// The node's parameters, the same for every kind.
    Parameters,
    /// A part of the node's body, named by its kind's GUI.
    Gui(&'static str),
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Part::Parameters => "parameters",
            Part::Gui(name) => name,
        }
    }
}

impl Popped {
    pub fn title(self, node: &Node) -> String {
        format!("{} {} · Drip", node.label, self.part.name())
    }

    /// The window's initial size, in points.
    pub fn size(self, node: &Node) -> Vec2 {
        match self.part {
            Part::Parameters => vec2(360.0, 80.0 + 26.0 * node.kind.params.len() as f32),
            Part::Gui(name) => of(node.kind).window_size(name, node),
        }
    }

    /// Draws the window's content.
    pub fn show(self, ui: &mut Ui, node: &mut NodeCx) {
        match self.part {
            Part::Parameters => inspector::node(ui, node),
            Part::Gui(name) => of(node.node().kind).window(ui, name, node),
        }
    }
}

/// A button over `rect` that pops `part` of the node out, or back in.
pub fn pop_out(ui: &mut Ui, rect: Rect, node: &mut NodeCx, part: Part) {
    let popped = node.popped(part);
    let icon = match part {
        Part::Parameters => "⚙",
        Part::Gui(_) => "🗗",
    };
    let button = egui::Button::new(icon).frame(false).selected(popped);
    let hint = if popped { "close its window" } else { "show in its own window" };
    if ui.put(rect, button).on_hover_text(format!("{}: {hint}", part.name())).clicked() {
        node.toggle(part);
    }
}

struct Plain;

impl NodeGui for Plain {}

/// Draws the node's view at a size the user sets from the body's corner; the
/// view pops out into a window filled by it.
struct Viewer;

/// Default size of a view, in graph units.
const VIEW: Vec2 = vec2(280.0, 190.0);
const SHOWN: Part = Part::Gui("view");

impl NodeGui for Viewer {
    fn size(&self, node: &Node) -> Vec2 {
        pair(&node.ui["size"]).unwrap_or(VIEW) + vec2(0.0, PAD)
    }

    fn body(&self, ui: &mut Ui, node: &mut NodeCx) {
        let body = ui.max_rect();
        let rect = Rect::from_min_max(body.min + vec2(PAD, 0.0), body.max - vec2(PAD, PAD));
        if node.popped(SHOWN) {
            let layout = egui::Layout::centered_and_justified(egui::Direction::TopDown);
            ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
                ui.weak("shown in its window")
            });
        } else if let Some(view) = node.view() {
            let font = FontId::proportional(11.0);
            views::draw(ui.painter(), rect, ui.id().with("view"), view, &font);
        }
        // Over the view, so on a backdrop.
        let button = Rect::from_min_size(rect.right_top() - vec2(BUTTON, 0.0), Vec2::splat(BUTTON));
        ui.painter().rect_filled(button, 0.0, theme::DARKER);
        pop_out(ui, button, node, SHOWN);
        if let Some(size) = resize(ui, body) {
            node.set_ui("size", (size - vec2(0.0, PAD)).max(vec2(80.0, 60.0)));
        }
    }

    fn window_size(&self, _name: &'static str, node: &Node) -> Vec2 {
        pair(&node.ui["size"]).unwrap_or(VIEW)
    }

    fn window(&self, ui: &mut Ui, _name: &'static str, node: &mut NodeCx) {
        if let Some(view) = node.view() {
            let (rect, font) = (ui.available_rect_before_wrap(), FontId::proportional(13.0));
            views::draw(ui.painter(), rect, ui.id().with("view"), view, &font);
        }
    }
}

/// A handle in the bottom-right corner of `rect` that resizes it; returns the
/// new size while dragged.
pub fn resize(ui: &mut Ui, rect: Rect) -> Option<Vec2> {
    let corner = Rect::from_min_max(rect.max - Vec2::splat(10.0), rect.max);
    let scale = ui.ctx().layer_transform_to_global(ui.layer_id()).map_or(1.0, |t| t.scaling);
    ui.painter().line_segment(
        [corner.left_bottom(), corner.right_top()],
        Stroke::new(1.0 / scale, theme::WEAK),
    );
    let handle = ui.interact(corner, ui.id().with("resize"), Sense::drag());
    handle.dragged().then(|| rect.size() + handle.drag_delta())
}

/// What one frame of node GUIs reads from the app, and what they did.
pub struct Frame<'a> {
    pub results: &'a dyn Fn(NodeId) -> Option<&'a Presentation>,
    /// The windows popped out.
    pub popped: &'a BTreeSet<Popped>,
    /// Disables actions while one runs.
    pub action_running: bool,
    pub report: Report,
}

/// What node GUIs did in one frame, for the app to apply.
#[derive(Default)]
pub struct Report {
    /// Whether the graph was edited.
    pub edited: bool,
    /// An edit the graph refused.
    pub refused: Option<String>,
    /// An action the user asked to run.
    pub action: Option<(NodeId, &'static str)>,
    /// A window the user opened or closed.
    pub toggled: Option<Popped>,
}

impl<'a> Frame<'a> {
    pub fn new(
        results: &'a dyn Fn(NodeId) -> Option<&'a Presentation>,
        popped: &'a BTreeSet<Popped>,
        action_running: bool,
    ) -> Self {
        Frame { results, popped, action_running, report: Report::default() }
    }

    pub fn set_param(&mut self, graph: &mut Graph, id: NodeId, name: &str, value: Json) {
        match graph.set_param(id, name, value) {
            Ok(()) => self.report.edited = true,
            Err(e) => self.report.refused = Some(e.to_string()),
        }
    }
}

/// One node as its GUI sees it. Edits go through the graph's validated
/// operations and are reported to the frame.
pub struct NodeCx<'g, 'f, 'a> {
    id: NodeId,
    graph: &'g mut Graph,
    frame: &'f mut Frame<'a>,
}

impl<'g, 'f, 'a> NodeCx<'g, 'f, 'a> {
    pub fn new(graph: &'g mut Graph, id: NodeId, frame: &'f mut Frame<'a>) -> Self {
        NodeCx { id, graph, frame }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn node(&self) -> &Node {
        self.graph.node(self.id).expect("GUIs are shown for existing nodes")
    }

    /// The view of the node's latest result, if it has one.
    pub fn view(&self) -> Option<&'a PreparedView> {
        (self.frame.results)(self.id)?.as_ref().ok()?.as_ref()
    }

    pub fn set_param(&mut self, name: &str, value: Json) {
        self.frame.set_param(self.graph, self.id, name, value);
    }

    pub fn set_label(&mut self, label: &str) {
        if let Err(e) = self.graph.set_label(self.id, label) {
            self.frame.report.refused = Some(e.to_string());
        }
    }

    pub fn set_external(&mut self, name: &str, external: bool) {
        self.graph.set_external(self.id, name, external).expect("the kind's parameter");
    }

    /// Sets `key` in the node's ui object to `value`, keeping the rest of it.
    pub fn set_ui(&mut self, key: &str, value: Vec2) {
        set_ui(self.graph, self.id, key, value);
    }

    pub fn run(&mut self, action: &'static str) {
        self.frame.report.action = Some((self.id, action));
    }

    /// Whether actions are disabled because one runs.
    pub fn action_running(&self) -> bool {
        self.frame.action_running
    }

    /// Whether `part` of the node is shown in its own window.
    pub fn popped(&self, part: Part) -> bool {
        self.frame.popped.contains(&Popped { node: self.id, part })
    }

    /// Opens the window of `part`, or closes it if open.
    pub fn toggle(&mut self, part: Part) {
        self.frame.report.toggled = Some(Popped { node: self.id, part });
    }
}

/// Sets `key` in a node's ui object to `value`, keeping the rest of it.
pub fn set_ui(graph: &mut Graph, id: NodeId, key: &str, value: Vec2) {
    let mut ui = graph.node(id).expect("node exists").ui.clone();
    if !ui.is_object() {
        ui = json!({});
    }
    ui[key] = json!([value.x, value.y]);
    graph.set_ui(id, ui).expect("node exists");
}

/// A two-number array in a node's ui object.
pub fn pair(value: &Json) -> Option<Vec2> {
    match value.as_array()?.iter().map(|v| v.as_f64()).collect::<Option<Vec<_>>>()?.as_slice() {
        [x, y] => Some(vec2(*x as f32, *y as f32)),
        _ => None,
    }
}
