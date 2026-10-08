//! Typed node implementations are erased only after binding to their declaration.

use super::ControlCx;
use crate::render::node_views::{Drawable, ImageCache, IntoDrawable};
use drip::eval::{Evaluation, NodeError};
use drip::graph::NodeId;
use drip::node::{EvalContext, KernelError, NodeDeclaration, NodeKind, TypedNode};
use drip::ports::InputTuple;
use egui::Ui;
use std::sync::Arc;

pub type Prepare<G> = for<'i, 'c, 'r> fn(
    <<G as GuiNode>::Node as NodeDeclaration>::Parameters,
    <<<G as GuiNode>::Node as NodeDeclaration>::Inputs as InputTuple>::Borrowed<'i>,
    &'c EvalContext<'r>,
) -> Result<<G as GuiNode>::Presentation, KernelError>;

/// Preparation has the declaration's exact signature. Controls can run without
/// inputs; preparation consumes one immutable evaluation snapshot. Runtime
/// refinements used by presentation are checked inside PREPARE, because requesting
/// inputs does not execute the declaration's numerical kernel or its checks.
pub trait GuiNode: Sized + 'static {
    type Node: NodeDeclaration;
    type Presentation: IntoDrawable;
    const NODE: &'static TypedNode<Self::Node>;
    const PREPARE: Option<Prepare<Self>> = None;

    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Node>) {
        node.schema(ui);
    }
}

type PrepareErased =
    fn(&Evaluation, NodeId, &mut ImageCache) -> Result<Arc<dyn Drawable>, NodeError>;

pub struct Binding {
    kind: &'static NodeKind,
    controls: fn(&mut Ui, &mut crate::editing::NodeCx),
    prepare: Option<PrepareErased>,
}

impl Binding {
    pub const fn new<G: GuiNode>() -> Self {
        Self {
            kind: G::NODE.kind(),
            controls: |ui, node| G::controls(ui, &mut ControlCx::new(node)),
            prepare: if G::PREPARE.is_some() {
                Some(|evaluation, id, cache| {
                    let value = evaluation.with_inputs(
                        id,
                        G::NODE,
                        G::PREPARE.expect("declared preparation"),
                    )?;
                    Ok(value.into_drawable(cache))
                })
            } else {
                None
            },
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
    pub fn prepare(
        &self,
        evaluation: &Evaluation,
        id: NodeId,
        cache: &mut ImageCache,
    ) -> Result<Option<Arc<dyn Drawable>>, NodeError> {
        self.prepare.map(|prepare| prepare(evaluation, id, cache)).transpose()
    }
}
