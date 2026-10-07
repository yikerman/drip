//! Typed node implementations are erased only after binding to their declaration.

use super::{ControlCx, NodeView};
use crate::render::node_views::{IntoPrepared, Prepared, PreparedView};
use drip::eval::{Evaluator, NodeError};
use drip::graph::{Graph, NodeId};
use drip::node::{EvalContext, KernelError, NodeDeclaration, NodeKind, TypedNode};
use drip::ports::InputTuple;
use egui::Ui;
use std::sync::Arc;

pub type Prepare<G> = for<'i, 'c, 'r> fn(
    <<G as GuiNode>::Node as NodeDeclaration>::Parameters,
    <<<G as GuiNode>::Node as NodeDeclaration>::Inputs as InputTuple>::Borrowed<'i>,
    &'c EvalContext<'r>,
) -> Result<<G as GuiNode>::Prepared, KernelError>;

/// Preparation has the declaration's exact signature. Controls can run without
/// inputs; preparation runs on the worker only for bindings that provide it.
pub trait GuiNode: Sized + 'static {
    type Node: NodeDeclaration;
    type Prepared: IntoPrepared;
    const NODE: &'static TypedNode<Self::Node>;
    const PREPARE: Option<Prepare<Self>> = None;
    const VIEW: Option<&'static dyn NodeView> = None;

    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Node>) {
        node.schema(ui);
    }
}

type PrepareErased = fn(
    &mut Evaluator,
    &Graph,
    NodeId,
    u8,
    &mut Prepared,
) -> Result<Arc<dyn PreparedView>, NodeError>;

pub struct Binding {
    kind: &'static NodeKind,
    controls: fn(&mut Ui, &mut crate::editing::NodeCx),
    prepare: Option<PrepareErased>,
    view: Option<&'static dyn NodeView>,
}

impl Binding {
    pub const fn new<G: GuiNode>() -> Self {
        Self {
            kind: G::NODE.kind(),
            controls: |ui, node| G::controls(ui, &mut ControlCx::new(node)),
            prepare: if G::PREPARE.is_some() {
                Some(|evaluator, graph, id, level, cache| {
                    let value = evaluator.with_inputs(
                        graph,
                        id,
                        level,
                        G::NODE,
                        G::PREPARE.expect("declared preparation"),
                    )?;
                    Ok(value.prepare(cache))
                })
            } else {
                None
            },
            view: G::VIEW,
        }
    }

    pub fn kind(&self) -> &'static NodeKind {
        self.kind
    }
    pub fn has_preparation(&self) -> bool {
        self.prepare.is_some()
    }
    pub fn controls(&self, ui: &mut Ui, node: &mut crate::editing::NodeCx) {
        (self.controls)(ui, node);
    }
    pub fn view(&self) -> Option<&'static dyn NodeView> {
        self.view
    }
    pub fn prepare(
        &self,
        evaluator: &mut Evaluator,
        graph: &Graph,
        id: NodeId,
        level: u8,
        cache: &mut Prepared,
    ) -> Result<Option<Arc<dyn PreparedView>>, NodeError> {
        self.prepare.map(|prepare| prepare(evaluator, graph, id, level, cache)).transpose()
    }
}

impl PreparedView for () {
    fn draw(&self, _: &egui::Painter, _: egui::Rect, _: egui::Id, _: &egui::FontId) {}
}
impl IntoPrepared for () {
    fn prepare(self, _: &mut Prepared) -> Arc<dyn PreparedView> {
        Arc::new(())
    }
}
