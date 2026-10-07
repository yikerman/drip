//! Parameter access stays typed until the node's frontend binding is erased.

use super::NodeView;
use crate::editing::NodeCx;
use drip::node::{NodeKind, TypedNode};
use drip::param::{Parameters, Params};
use egui::Ui;
use std::marker::PhantomData;

pub struct ControlCx<'n, 'g, 'f, 'a, P> {
    node: &'n mut NodeCx<'g, 'f, 'a>,
    parameters: PhantomData<fn() -> P>,
}

impl<P: Parameters> ControlCx<'_, '_, '_, '_, P> {
    /// Read after drawing controls so same-frame edits also reach custom plots.
    pub fn parameters(&self) -> P {
        P::read(Params::validated(&self.node.node().params))
    }

    pub fn schema(&mut self, ui: &mut Ui) {
        super::parameters::schema(ui, self.node);
    }
}

pub struct Controls<P> {
    draw: fn(&mut Ui, &mut ControlCx<'_, '_, '_, '_, P>),
}

impl<P> Controls<P> {
    pub const fn new(draw: fn(&mut Ui, &mut ControlCx<'_, '_, '_, '_, P>)) -> Self {
        Self { draw }
    }
}

pub trait ErasedControls: Sync {
    fn show(&self, ui: &mut Ui, node: &mut NodeCx);
}

impl<P: Parameters> ErasedControls for Controls<P> {
    fn show(&self, ui: &mut Ui, node: &mut NodeCx) {
        (self.draw)(ui, &mut ControlCx { node, parameters: PhantomData });
    }
}

pub(super) struct Schema;
impl ErasedControls for Schema {
    fn show(&self, ui: &mut Ui, node: &mut NodeCx) {
        super::parameters::schema(ui, node);
    }
}

/// Custom presentation is declared beside its implementation in this crate.
pub(crate) struct Binding {
    kind: &'static NodeKind,
    controls: Option<&'static dyn ErasedControls>,
    view: Option<&'static dyn NodeView>,
}

impl Binding {
    pub(super) fn kind(&self) -> &'static NodeKind {
        self.kind
    }
    pub(super) fn controls(&self) -> Option<&'static dyn ErasedControls> {
        self.controls
    }
    pub(super) fn view(&self) -> Option<&'static dyn NodeView> {
        self.view
    }

    pub const fn new<P: Parameters + 'static>(
        node: &'static TypedNode<P>,
        controls: Option<&'static Controls<P>>,
        view: Option<&'static dyn NodeView>,
    ) -> Self {
        Self {
            kind: node.kind(),
            controls: match controls {
                Some(controls) => Some(controls),
                None => None,
            },
            view,
        }
    }
}
