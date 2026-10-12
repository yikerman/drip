//! Optional node-specific presentation; ordinary nodes use schema controls.

mod binding;
mod controls;
pub(crate) mod exposure;
pub mod help;
pub mod preview;
pub mod scopes;
pub use binding::{Action, Binding, GuiNode, Prepare};
pub use controls::ControlCx;
mod image_view;
pub mod parameters;
pub mod ports;
mod sigmoid;
mod viewer;
use crate::editing::NodeCx;
use crate::model::Kind;
use crate::model::{Node, NodeId};
use egui::{Rect, Ui, Vec2, vec2};

/// Width of a node whose body does not set one, in graph units.
pub const WIDTH: f32 = 200.0;
/// Space around a node's contents, in graph units.
pub const PAD: f32 = 6.0;

/// Nodes that only present their inputs; what they prepare is GUI-only.
pub static VIEWS: &[&Kind] = &[
    &preview::observe::KIND,
    &scopes::histogram::observe::KIND,
    &scopes::waveform::observe::KIND,
    &scopes::vectorscope::observe::KIND,
];

#[linkme::distributed_slice]
pub(super) static BINDINGS: [Binding];

/// Whether this kind prepares a view alongside its controls.
pub fn has_view(kind: &Kind) -> bool {
    binding(kind).is_some_and(Binding::has_preparation)
}

pub fn size(node: &Node) -> Vec2 {
    if has_view(node.kind) { viewer::size(node) + vec2(0.0, PAD) } else { vec2(WIDTH, PAD) }
}

pub fn body(ui: &mut Ui, node: &mut NodeCx) {
    if has_view(node.kind()) {
        viewer::body(ui, node);
    }
}

pub fn controls(ui: &mut Ui, node: &mut NodeCx) {
    match binding(node.kind()) {
        Some(binding) => binding.controls(ui, node),
        None => parameters::schema(ui, node),
    }
}

pub fn binding(kind: &Kind) -> Option<&'static Binding> {
    use std::{collections::BTreeMap, sync::LazyLock};
    static CUSTOM: LazyLock<BTreeMap<&'static str, &'static Binding>> = LazyLock::new(|| {
        let mut bindings = BTreeMap::new();
        for binding in BINDINGS {
            let id = binding.kind().id;
            assert!(bindings.insert(id, binding).is_none(), "duplicate node UI: {id}");
            assert!(
                binding.action().is_none() || binding.kind().action.is_none(),
                "{id} has a frontend and a node action"
            );
        }
        bindings
    });
    CUSTOM.get(kind.id).copied().filter(|binding| std::ptr::eq(binding.kind(), kind))
}

/// A node kind's action: a frontend extra, or the action declared with the
/// node. A kind never has both.
#[derive(Clone, Copy)]
pub struct NodeAction {
    pub name: &'static str,
    run: Run,
}
#[derive(Clone, Copy)]
enum Run {
    Frontend(binding::ActionErased),
    Node(fn(&dyn drip::definition::Node, &drip::eval::InputValues) -> drip::Result<()>),
}
pub fn action(kind: &'static Kind) -> Option<NodeAction> {
    match binding(kind).and_then(Binding::action) {
        Some((name, run)) => Some(NodeAction { name, run: Run::Frontend(run) }),
        None => kind.action.map(|a| NodeAction { name: a.name, run: Run::Node(a.run) }),
    }
}
impl NodeAction {
    /// Runs on a node of the action's kind; a frontend action may return new
    /// node parameters.
    pub fn run(
        self,
        node: &dyn drip::definition::Node,
        inputs: &drip::eval::InputValues,
        ctx: &data::PrepareContext,
    ) -> drip::Result<Option<serde_json::Value>> {
        match self.run {
            Run::Frontend(run) => run(node, inputs, ctx),
            Run::Node(run) => run(node, inputs).map(|()| None),
        }
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
    /// The prepared view.
    View,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Parameters => "Parameters",
            Part::View => "View",
        }
    }
}

impl Popped {
    pub fn title(self, name: &str) -> String {
        format!("{name} · {} · Drip", self.part.name())
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

pub mod data;
pub mod templates;
