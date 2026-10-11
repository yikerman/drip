//! Frontend extras bound to a declared node type: custom controls, a prepared
//! view and an action. Kind, parameters and typed inputs come
//! from the node's own declaration.
use super::ControlCx;
use crate::{
    model::Kind,
    node_ui::data::PrepareContext,
    render::node_views::{Drawable, ImageCache, IntoDrawable},
};
use drip::{
    Result,
    definition::{Node, Typed},
    eval::InputValues,
    param::encode,
};
use egui::Ui;
use serde_json::Value;
use std::sync::Arc;

type Parameters<G> = <G as Typed>::Parameters;
type Inputs<'a, G> = <G as Typed>::Inputs<'a>;
/// Runs on the admitted node, which holds its parameters and any assets.
pub type Prepare<G> =
    for<'a> fn(&G, Inputs<'a, G>, &PrepareContext) -> Result<<G as GuiNode>::Presentation>;
type Run<G> = for<'a> fn(&G, Inputs<'a, G>, &PrepareContext) -> Result<Option<Parameters<G>>>;
/// A named frontend action. It may return new parameters for its node, which
/// the app applies as an ordinary edit once the action finishes.
pub struct Action<G: Typed> {
    pub name: &'static str,
    pub run: Run<G>,
}

/// Implemented on a node's generated `Definition` with `#[gui_node]`.
pub trait GuiNode: Typed {
    type Presentation: IntoDrawable;
    const PREPARE: Option<Prepare<Self>> = None;
    const ACTION: Option<Action<Self>> = None;
    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        node.schema(ui);
    }
}
type PrepareErased =
    fn(&dyn Node, &InputValues, &PrepareContext, &mut ImageCache) -> Result<Arc<dyn Drawable>>;
pub(super) type ActionErased =
    fn(&dyn Node, &InputValues, &PrepareContext) -> Result<Option<Value>>;
pub struct Binding {
    kind: &'static Kind,
    controls: fn(&mut Ui, &mut crate::editing::NodeCx),
    prepare: Option<PrepareErased>,
    action: Option<(&'static str, ActionErased)>,
}
impl Binding {
    pub const fn new<G: GuiNode>() -> Self {
        Self {
            kind: G::KIND,
            controls: |ui, node| G::controls(ui, &mut ControlCx::new(node)),
            // Erased extras are function pointers, which cannot capture: each
            // reads its declaration again.
            prepare: match G::PREPARE {
                Some(_) => Some(|node, inputs, ctx, cache| {
                    let prepare = G::PREPARE.expect("declared preparation");
                    Ok(prepare(G::of(node)?, inputs.bind::<G>()?, ctx)?.into_drawable(cache))
                }),
                None => None,
            },
            action: match G::ACTION {
                Some(action) => Some((action.name, |node, inputs, ctx| {
                    let run = G::ACTION.expect("declared action").run;
                    run(G::of(node)?, inputs.bind::<G>()?, ctx)?.as_ref().map(encode).transpose()
                })),
                None => None,
            },
        }
    }
    pub fn kind(&self) -> &'static Kind {
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
        node: &dyn Node,
        inputs: &InputValues,
        ctx: &PrepareContext,
        cache: &mut ImageCache,
    ) -> Result<Option<Arc<dyn Drawable>>> {
        self.prepare.map(|prepare| prepare(node, inputs, ctx, cache)).transpose()
    }
    pub(super) fn action(&self) -> Option<(&'static str, ActionErased)> {
        self.action
    }
}
