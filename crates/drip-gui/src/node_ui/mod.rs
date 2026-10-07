//! Optional node-specific presentation; ordinary nodes use schema controls.

mod binding;
mod controls;
pub mod help;
pub mod preview;
pub mod scopes;
pub use binding::{Binding, GuiNode, Prepare};
pub use controls::ControlCx;
mod image_view;
pub mod parameters;
pub mod ports;
mod sigmoid;
mod viewer;
use crate::editing::NodeCx;
use drip::graph::{Node, NodeId};
use drip::node::NodeKind;
use egui::{Rect, Ui, Vec2, vec2};

/// Width of a node whose body does not set one, in graph units.
pub const WIDTH: f32 = 160.0;
/// Space around a node's contents, in graph units.
pub const PAD: f32 = 6.0;

#[linkme::distributed_slice]
pub(super) static BINDINGS: [Binding];

/// Custom controls coexist with the standard viewer whenever preparation exists.
pub struct NodeUi {
    binding: Option<&'static Binding>,
}
impl NodeUi {
    fn has_view(&self) -> bool {
        self.binding.is_some_and(Binding::has_preparation)
    }

    pub fn size(&self, node: &Node) -> Vec2 {
        if self.has_view() { viewer::size(node) + vec2(0.0, PAD) } else { vec2(WIDTH, PAD) }
    }

    pub fn body(&self, ui: &mut Ui, node: &mut NodeCx) {
        if self.has_view() {
            viewer::body(ui, node);
        }
    }

    pub fn controls(&self, ui: &mut Ui, node: &mut NodeCx) {
        match self.binding {
            Some(binding) => binding.controls(ui, node),
            None => parameters::schema(ui, node),
        }
    }
}

pub fn binding(kind: &NodeKind) -> Option<&'static Binding> {
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
    CUSTOM.get(kind.id).copied().filter(|binding| std::ptr::eq(binding.kind(), kind))
}

pub fn of(kind: &NodeKind) -> NodeUi {
    NodeUi { binding: binding(kind) }
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
    /// The prepared view.
    View,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Parameters => "parameters",
            Part::View => "view",
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
            Part::View => viewer::size(node),
        }
    }

    /// Draws the window's content.
    pub fn show(self, ui: &mut Ui, node: &mut NodeCx) {
        match self.part {
            Part::Parameters => {
                let used = ui.scope(|ui| parameters::panel(ui, node)).response.rect;
                fit_window(ui.ctx(), used);
            }
            Part::View => viewer::window(ui, node),
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
