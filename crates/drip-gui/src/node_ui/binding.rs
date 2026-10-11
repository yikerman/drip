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
    definition::Typed,
    eval::InputValues,
    param::{decode, encode},
};
use egui::Ui;
use serde_json::Value;
use std::sync::Arc;

type Parameters<G> = <G as Typed>::Parameters;
type Inputs<'a, G> = <G as Typed>::Inputs<'a>;
pub type Prepare<G> = for<'a> fn(
    Parameters<G>,
    Inputs<'a, G>,
    &PrepareContext,
) -> Result<<G as GuiNode>::Presentation>;
type Run<G> =
    for<'a> fn(Parameters<G>, Inputs<'a, G>, &PrepareContext) -> Result<Option<Parameters<G>>>;
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
    fn(Value, &InputValues, &PrepareContext, &mut ImageCache) -> Result<Arc<dyn Drawable>>;
type ActionErased = fn(Value, &InputValues, &PrepareContext) -> Result<Option<Value>>;
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
            prepare: if G::PREPARE.is_some() {
                Some(|params, inputs, ctx, cache| {
                    let prepare = G::PREPARE.expect("declared preparation");
                    Ok(prepare(decode(params)?, inputs.bind::<G>()?, ctx)?.into_drawable(cache))
                })
            } else {
                None
            },
            action: match G::ACTION {
                Some(action) => Some((action.name, |params, inputs, ctx| {
                    let run = G::ACTION.expect("declared action").run;
                    run(decode(params)?, inputs.bind::<G>()?, ctx)?.as_ref().map(encode).transpose()
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
        params: Value,
        inputs: &InputValues,
        ctx: &PrepareContext,
        cache: &mut ImageCache,
    ) -> Result<Option<Arc<dyn Drawable>>> {
        self.prepare.map(|prepare| prepare(params, inputs, ctx, cache)).transpose()
    }
    pub fn action_name(&self) -> Option<&'static str> {
        self.action.map(|(name, _)| name)
    }
    pub fn run_action(
        &self,
        name: &str,
        params: Value,
        inputs: &InputValues,
        ctx: &PrepareContext,
    ) -> Result<Option<Value>> {
        let (_, action) = self
            .action
            .filter(|(declared, _)| name == *declared)
            .ok_or_else(|| drip::Error::Graph("unknown node action".into()))?;
        action(params, inputs, ctx)
    }
}
