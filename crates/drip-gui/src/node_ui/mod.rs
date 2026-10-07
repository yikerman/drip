//! Optional node-specific presentation; ordinary nodes use schema controls.

mod controls;
pub mod help;
pub(super) use controls::Binding;
pub use controls::{ControlCx, Controls};
mod image_view;
pub mod parameters;
pub mod ports;
mod sigmoid;
mod viewer;
use crate::editing::NodeCx;
use drip::graph::{Node, NodeId};
use drip::node::NodeKind;
use egui::{Rect, Ui, Vec2, vec2};
use viewer::Viewer;

/// Width of a node whose body does not set one, in graph units.
pub const WIDTH: f32 = 160.0;
/// Space around a node's contents, in graph units.
pub const PAD: f32 = 6.0;

/// Canvas body and its pop-out views, independent of parameter controls.
pub trait NodeView: Sync {
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

#[linkme::distributed_slice]
pub(super) static BINDINGS: [Binding];

/// Resolved controls and view; overriding either preserves the other's default.
pub struct NodeUi {
    pub controls: &'static dyn controls::ErasedControls,
    pub view: &'static dyn NodeView,
}

pub fn of(kind: &NodeKind) -> NodeUi {
    use std::{collections::BTreeMap, sync::LazyLock};
    static CUSTOM: LazyLock<BTreeMap<&'static str, &'static Binding>> = LazyLock::new(|| {
        let mut bindings = BTreeMap::new();
        for binding in BINDINGS {
            assert!(
                bindings.insert(binding.kind().id, binding).is_none(),
                "duplicate node UI: {}",
                binding.kind().id
            );
        }
        bindings
    });
    let custom = CUSTOM.get(kind.id);
    NodeUi {
        controls: custom.and_then(|binding| binding.controls()).unwrap_or(&controls::Schema),
        view: custom.and_then(|binding| binding.view()).unwrap_or(if kind.has_view() {
            &Viewer
        } else {
            &Plain
        }),
    }
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
    pub fn name(self) -> &'static str {
        match self {
            Part::Parameters => "parameters",
            Part::Gui(name) => name,
        }
    }
}

impl Popped {
    pub fn title(self, node: &Node) -> String {
        format!("{} {} · Drip", node.name, self.part.name())
    }

    /// The window's size, in points, when its content does not ask for one.
    pub fn size(self, node: &Node) -> Vec2 {
        match self.part {
            Part::Parameters => vec2(360.0, 240.0),
            Part::Gui(name) => of(node.kind).view.window_size(name, node),
        }
    }

    /// Draws the window's content.
    pub fn show(self, ui: &mut Ui, node: &mut NodeCx) {
        match self.part {
            Part::Parameters => {
                let used = ui.scope(|ui| parameters::panel(ui, node)).response.rect;
                fit_window(ui.ctx(), used);
            }
            Part::Gui(name) => of(node.node().kind).view.window(ui, name, node),
        }
    }
}

/// Records a window size that fits `used`, the rect the window's content
/// took, for `App::window_size` to open the window at. Passes egui discards
/// are not laid out for real, so they record nothing.
fn fit_window(ctx: &egui::Context, used: Rect) {
    if !ctx.will_discard() {
        // The panel's margins are equal on all sides.
        let size = used.max.to_vec2() + used.min.to_vec2();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(FITTED), size));
    }
}

const FITTED: &str = "fitted window size";

/// The window size the content last recorded in `ctx`, if it fits itself.
pub fn fitted(ctx: &egui::Context) -> Option<Vec2> {
    ctx.data(|d| d.get_temp(egui::Id::new(FITTED)))
}

struct Plain;

impl NodeView for Plain {}
