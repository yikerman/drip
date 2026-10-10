//! Parameter access stays typed until the node's frontend binding is erased.

use crate::editing::NodeCx;
use serde::de::DeserializeOwned;

use egui::Ui;
use std::marker::PhantomData;

/// Presentation hints owned by a node's GUI, shared by all parameter surfaces.
#[derive(Clone, Copy, Default)]
pub struct ParameterUi {
    pub file: Option<FileUi>,
}

#[derive(Clone, Copy)]
pub struct FileUi {
    pub title: &'static str,
    pub filter: &'static str,
    pub extensions: &'static [&'static str],
}

pub struct ControlCx<'n, 'g, 'f, 'a, N: DeserializeOwned> {
    node: &'n mut NodeCx<'g, 'f, 'a>,
    parameters: PhantomData<fn() -> N>,
}

impl<'n, 'g, 'f, 'a, N: DeserializeOwned> ControlCx<'n, 'g, 'f, 'a, N> {
    pub(super) fn new(node: &'n mut NodeCx<'g, 'f, 'a>) -> Self {
        Self { node, parameters: PhantomData }
    }

    /// Read after drawing controls so same-frame edits also reach custom plots.
    pub fn parameters(&self) -> N {
        serde_json::from_value(self.node.node().params).expect("validated node parameters")
    }

    pub fn schema(&mut self, ui: &mut Ui) {
        super::parameters::schema(ui, self.node);
    }
}
