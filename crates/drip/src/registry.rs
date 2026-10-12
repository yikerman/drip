//! The registries: every node kind the library provides, listed by hand. A
//! frontend composes a `Registry` from these and its own lists; see the crate
//! README.
use crate::{
    Error, Result,
    definition::{Kind, Node},
    node::{
        bayer_2x2, bayer_gains, camera_to_rgb, chromatic_adaptation, export, exposure, highlights,
        orientation, raw, rcd, reduce, sigmoid,
    },
};
use std::sync::Arc;

/// Every processing node kind, in pipeline order. A kind missing here cannot
/// be added or loaded from a project.
pub static NODES: &[&Kind] = &[
    &raw::read::KIND,
    &reduce::bayer::compute::KIND,
    &bayer_gains::compute::KIND,
    &highlights::reconstruct::KIND,
    &bayer_2x2::compute::KIND,
    &rcd::compute::KIND,
    &camera_to_rgb::compute::KIND,
    &chromatic_adaptation::compute::KIND,
    &orientation::compute::KIND,
    &reduce::rgb::compute::KIND,
    &exposure::compute::KIND,
    &sigmoid::apply::KIND,
    &export::tiff::KIND,
];

/// The node kinds a frontend offers and loads, in list order.
pub struct Registry(Vec<&'static Kind>);
impl Registry {
    pub fn new(lists: &[&[&'static Kind]]) -> Self {
        let kinds: Vec<_> = lists.concat();
        for (i, kind) in kinds.iter().enumerate() {
            assert!(kinds[..i].iter().all(|k| k.id != kind.id), "duplicate node ID {}", kind.id);
        }
        Self(kinds)
    }
    pub fn kind(&self, id: &str) -> Option<&'static Kind> {
        self.0.iter().copied().find(|kind| kind.id == id)
    }
    pub fn kinds(&self) -> impl Iterator<Item = &'static Kind> + '_ {
        self.0.iter().copied()
    }
    /// Admit persisted parameters for a listed kind.
    pub fn build(&self, id: &str, parameters: serde_json::Value) -> Result<Arc<dyn Node>> {
        let kind = self.kind(id).ok_or_else(|| Error::Graph(format!("unknown node {id}")))?;
        (kind.build)(parameters)
    }
}
