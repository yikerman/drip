//! Parameter access stays typed until the node's frontend binding is erased.

use crate::editing::NodeCx;
use drip::node::NodeDeclaration;
use drip::param::{Parameters, Params};
use egui::Ui;
use std::marker::PhantomData;

pub struct ControlCx<'n, 'g, 'f, 'a, N: NodeDeclaration> {
    node: &'n mut NodeCx<'g, 'f, 'a>,
    parameters: PhantomData<fn() -> N>,
}

impl<'n, 'g, 'f, 'a, N: NodeDeclaration> ControlCx<'n, 'g, 'f, 'a, N> {
    pub(super) fn new(node: &'n mut NodeCx<'g, 'f, 'a>) -> Self {
        Self { node, parameters: PhantomData }
    }

    /// Read after drawing controls so same-frame edits also reach custom plots.
    pub fn parameters(&self) -> N::Parameters {
        N::Parameters::read(Params::validated(&self.node.node().params))
    }

    pub fn schema(&mut self, ui: &mut Ui) {
        super::parameters::schema(ui, self.node);
    }
}
