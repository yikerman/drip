//! Node GUIs (DESIGN G13): each node kind draws its own body on the canvas, in
//! graph units, through a context limited to that node. Kinds without a GUI of
//! their own get the default, an empty body.

use drip::graph::{Graph, Node, NodeId};
use drip::node::NodeKind;
use drip::nodes::{HISTOGRAM, PREVIEW};
use egui::{FontId, Rect, Sense, Stroke, Ui, Vec2, vec2};
use serde_json::{Value as Json, json};

use crate::theme;
use crate::views::{self, PreparedView};
use crate::worker::Presentation;

/// Width of a node whose body does not set one, in graph units.
pub const WIDTH: f32 = 160.0;
/// Space around a node's contents, in graph units.
pub const PAD: f32 = 6.0;

pub trait NodeGui: Sync {
    /// The size of the node's body, below its ports, in graph units. Its
    /// width is the node's.
    fn size(&self, _node: &Node) -> Vec2 {
        vec2(WIDTH, PAD)
    }

    /// Draws the body into `ui`, whose max rect has the body's size.
    fn body(&self, _ui: &mut Ui, _node: &mut NodeCx) {}
}

/// The GUI of nodes of `kind`.
pub fn of(kind: &NodeKind) -> &'static dyn NodeGui {
    static GUIS: &[(&NodeKind, &dyn NodeGui)] = &[(&PREVIEW, &Viewer), (&HISTOGRAM, &Viewer)];
    GUIS.iter().find(|(k, _)| *k == kind).map_or(&Plain, |(_, gui)| *gui)
}

struct Plain;

impl NodeGui for Plain {}

/// Draws the node's view at a size the user sets from the body's corner.
struct Viewer;

/// Default size of a view, in graph units.
const VIEW: Vec2 = vec2(280.0, 190.0);

impl NodeGui for Viewer {
    fn size(&self, node: &Node) -> Vec2 {
        pair(&node.ui["size"]).unwrap_or(VIEW) + vec2(0.0, PAD)
    }

    fn body(&self, ui: &mut Ui, node: &mut NodeCx) {
        let body = ui.max_rect();
        let rect = Rect::from_min_max(body.min + vec2(PAD, 0.0), body.max - vec2(PAD, PAD));
        if let Some(view) = node.view() {
            views::draw(
                ui.painter(),
                rect,
                ui.id().with("view"),
                view,
                &FontId::proportional(11.0),
            );
        }
        if let Some(size) = resize(ui, body) {
            node.set_ui("size", (size - vec2(0.0, PAD)).max(vec2(80.0, 60.0)));
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

/// What one frame of node GUIs reads from the app and reports back.
pub struct Frame<'a> {
    pub results: &'a dyn Fn(NodeId) -> Option<&'a Presentation>,
    /// Disables actions while one runs.
    pub action_running: bool,
    /// An edit the graph refused.
    pub refused: Option<String>,
    pub changed: bool,
    /// An action the user asked to run.
    pub action: Option<(NodeId, &'static str)>,
}

impl Frame<'_> {
    pub fn set_param(&mut self, graph: &mut Graph, id: NodeId, name: &str, value: Json) {
        match graph.set_param(id, name, value) {
            Ok(()) => self.changed = true,
            Err(e) => self.refused = Some(e.to_string()),
        }
    }
}

/// One node as its GUI sees it. Edits go through the graph's validated
/// operations and are reported to the frame.
pub struct NodeCx<'g, 'f, 'a> {
    pub id: NodeId,
    graph: &'g mut Graph,
    pub frame: &'f mut Frame<'a>,
}

impl<'g, 'f, 'a> NodeCx<'g, 'f, 'a> {
    pub fn new(graph: &'g mut Graph, id: NodeId, frame: &'f mut Frame<'a>) -> Self {
        NodeCx { id, graph, frame }
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
            self.frame.refused = Some(e.to_string());
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
        self.frame.action = Some((self.id, action));
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
