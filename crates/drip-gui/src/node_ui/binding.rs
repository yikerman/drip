//! Frontend extras bound to a declared node type: custom controls, a prepared
//! view and an action. Kind, parameters and typed inputs come
//! from the node's own declaration.
use super::ControlCx;
use crate::{
    model::Kind,
    node_ui::data::PrepareContext,
    render::node_views::{Drawable, Images, IntoDrawable},
};
use drip::{
    Result,
    definition::{Node, Typed},
    eval::{InputValues, TargetValues},
    param::encode,
};
use egui::Ui;
use serde_json::Value;
use std::sync::Arc;

type Parameters<G> = <G as Typed>::Parameters;
type Inputs<'a, G> = <G as Typed>::Inputs<'a>;
type Outputs<'a, G> = <G as Typed>::Outputs<'a>;
type Presentation<G> = <G as GuiNode>::Presentation;
/// Turns what a preview hands a node into its view. Runs on the admitted
/// node, which holds its parameters and any assets.
pub enum Prepare<G: GuiNode> {
    /// From the node's typed inputs: a view, or any node but a reduction.
    Inputs(for<'a> fn(&G, Inputs<'a, G>, &PrepareContext) -> Result<Presentation<G>>),
    /// From the typed outputs of a reduce or bins node, which evaluation
    /// hands in place of its inputs.
    Outputs(for<'a> fn(&G, Outputs<'a, G>, &PrepareContext) -> Result<Presentation<G>>),
}
type Run<G> = for<'a> fn(&G, Inputs<'a, G>, &PrepareContext) -> Result<Option<Parameters<G>>>;
/// A named frontend action. It may return new parameters for its node, which
/// the app applies as an ordinary edit once the action finishes.
pub struct Action<G: Typed> {
    pub name: &'static str,
    pub run: Run<G>,
}

/// Implemented on a node's generated `Definition` and listed in `registry::BINDINGS`.
pub trait GuiNode: Typed {
    type Presentation: IntoDrawable;
    const PREPARE: Option<Prepare<Self>> = None;
    const ACTION: Option<Action<Self>> = None;
    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        node.schema(ui);
    }
}
type PrepareErased =
    fn(&dyn Node, &TargetValues, &PrepareContext, &mut Images) -> Result<Arc<dyn Drawable>>;
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
                Some(_) => Some(|node, values, ctx, images| {
                    let node = G::of(node)?;
                    let presentation = match G::PREPARE.expect("declared preparation") {
                        Prepare::Inputs(prepare) => prepare(node, values.bind::<G>()?, ctx),
                        Prepare::Outputs(prepare) => {
                            prepare(node, values.bind_outputs::<G>()?, ctx)
                        }
                    };
                    Ok(presentation?.into_drawable(images))
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
        values: &TargetValues,
        ctx: &PrepareContext,
        images: &mut Images,
    ) -> Result<Option<Arc<dyn Drawable>>> {
        self.prepare.map(|prepare| prepare(node, values, ctx, images)).transpose()
    }
    pub(super) fn action(&self) -> Option<(&'static str, ActionErased)> {
        self.action
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn listed_bindings_are_unique_and_never_double_a_node_action() {
        let bindings: Vec<_> = crate::registry::BINDINGS.iter().copied().flatten().collect();
        for (i, binding) in bindings.iter().enumerate() {
            let id = binding.kind().id;
            assert!(bindings[..i].iter().all(|b| b.kind().id != id), "duplicate node UI: {id}");
            assert!(
                binding.action().is_none() || binding.kind().action.is_none(),
                "{id} has a frontend and a node action"
            );
        }
    }
}
