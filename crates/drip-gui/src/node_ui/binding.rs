//! Frontend extras bound to a declared node type: custom controls, a prepared
//! view, an action and parameter hints. Kind, parameters and typed inputs come
//! from the node's own declaration.
use super::{ControlCx, ParameterUi};
use crate::{
    model::Kind,
    node_ui::data::PrepareContext,
    render::node_views::{Drawable, ImageCache, IntoDrawable},
};
use drip::{Result, definition::Typed, eval::InputValues};
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
pub type Action<G> = for<'a> fn(Parameters<G>, Inputs<'a, G>, &PrepareContext) -> Result<()>;

/// Implemented on a node's generated `Definition` with `#[gui_node]`.
pub trait GuiNode: Typed {
    type Presentation: IntoDrawable;
    const PREPARE: Option<Prepare<Self>> = None;
    const ACTION: Option<Action<Self>> = None;
    const ACTION_NAME: &'static str = "Export";
    fn parameter_ui(_: &Self::Parameters, _: &str) -> ParameterUi {
        ParameterUi::default()
    }
    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        node.schema(ui);
    }
}
type PrepareErased =
    fn(Value, &InputValues, &PrepareContext, &mut ImageCache) -> Result<Arc<dyn Drawable>>;
type ActionErased = fn(Value, &InputValues, &PrepareContext) -> Result<()>;
pub struct Binding {
    kind: &'static Kind,
    controls: fn(&mut Ui, &mut crate::editing::NodeCx),
    prepare: Option<PrepareErased>,
    action: Option<ActionErased>,
    action_name: &'static str,
    parameter_ui: fn(Value, &str) -> ParameterUi,
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
            action: if G::ACTION.is_some() {
                Some(|params, inputs, ctx| {
                    G::ACTION.expect("declared action")(decode(params)?, inputs.bind::<G>()?, ctx)
                })
            } else {
                None
            },
            action_name: G::ACTION_NAME,
            parameter_ui: |params, name| {
                G::parameter_ui(&decode(params).expect("validated parameters"), name)
            },
        }
    }
    pub fn kind(&self) -> &'static Kind {
        self.kind
    }
    pub fn parameter_ui(&self, params: Value, name: &str) -> ParameterUi {
        (self.parameter_ui)(params, name)
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
        self.action.map(|_| self.action_name)
    }
    pub fn run_action(
        &self,
        name: &str,
        params: Value,
        inputs: &InputValues,
        ctx: &PrepareContext,
    ) -> Result<()> {
        let action = self
            .action
            .filter(|_| name == self.action_name)
            .ok_or_else(|| drip::Error::Graph("unknown node action".into()))?;
        action(params, inputs, ctx)
    }
}
fn decode<P: serde::de::DeserializeOwned>(params: Value) -> Result<P> {
    serde_json::from_value(params).map_err(|e| drip::Error::Contract(e.to_string()))
}
