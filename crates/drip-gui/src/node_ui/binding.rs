//! Optional typed controls, preparation and actions over declared node inputs.
use super::{ControlCx, ParameterUi};
use crate::{
    model::NodeKind,
    node_ui::data::PrepareContext,
    render::node_views::{Drawable, ImageCache, IntoDrawable},
};
use drip::{Result, eval::InputValues};
use egui::Ui;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;

pub type Prepare<G> = fn(
    <G as GuiNode>::Parameters,
    &InputValues,
    &PrepareContext,
) -> Result<<G as GuiNode>::Presentation>;
pub type Action<G> = fn(<G as GuiNode>::Parameters, &InputValues, &PrepareContext) -> Result<()>;
pub trait GuiNode: Sized + 'static {
    type Parameters: DeserializeOwned;
    type Presentation: IntoDrawable;
    const ID: &'static str;
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
    id: &'static str,
    controls: fn(&mut Ui, &mut crate::editing::NodeCx),
    prepare: Option<PrepareErased>,
    action: Option<ActionErased>,
    action_name: &'static str,
    parameter_ui: fn(Value, &str) -> ParameterUi,
}
impl Binding {
    pub const fn new<G: GuiNode>() -> Self {
        Self {
            id: G::ID,
            controls: |ui, node| G::controls(ui, &mut ControlCx::new(node)),
            prepare: if G::PREPARE.is_some() {
                Some(|params, inputs, ctx, cache| {
                    let params = decode(params)?;
                    Ok(G::PREPARE.expect("declared preparation")(params, inputs, ctx)?
                        .into_drawable(cache))
                })
            } else {
                None
            },
            action: if G::ACTION.is_some() {
                Some(|params, inputs, ctx| {
                    G::ACTION.expect("declared action")(decode(params)?, inputs, ctx)
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
    pub fn kind(&self) -> &'static NodeKind {
        crate::model::Registry.get(self.id).expect("GUI binds a registered node")
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
fn decode<P: DeserializeOwned>(params: Value) -> Result<P> {
    serde_json::from_value(params).map_err(|e| drip::Error::Contract(e.to_string()))
}
