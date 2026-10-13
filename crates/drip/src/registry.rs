//! Every node kind and tag the library provides, listed by hand.
//!
//! A kind missing from a list cannot be added or loaded, so a frontend states
//! what it knows by composing a [`Registry`] from these lists and its own.
//! Explicit lists read plainly and avoid link-time registration.
use crate::{
    Error, Result,
    definition::{Kind, Node},
    node::{
        bayer_2x2, bayer_gains, camera_to_rgb, chromatic_adaptation, export, exposure, highlights,
        hot_pixels, lens, orientation, perspective, pyramid, raw, rcd, reduce, resample, sharpen,
        sigmoid,
    },
};
use std::sync::Arc;

/// Tags nodes may carry, stages first. Tags say what a node is for, since
/// ports already say what it accepts. By convention a node takes at most one
/// of the ACES stages \[1\]: input transform, look or output transform.
/// Geometry, views and file writers take none, and nodes that read or write
/// files carry `io`. Frontends declare no tags of their own. Each tag states
/// how durable its nodes' outputs are in the store.
///
/// \[1\] Academy Color Encoding System, “ACES overview.” \[Online\].
/// Available: <https://docs.acescentral.com/background/overview>
pub mod tag {
    use crate::definition::{Durability::*, Tag};
    macro_rules! tags {
        ($($name:ident $id:literal $durability:ident $description:literal;)*) => {
            $(pub static $name: Tag =
                Tag { id: $id, description: $description, durability: $durability };)*
            /// Every tag, in display order.
            pub static TAGS: &[&Tag] = &[$(&$name),*];
        };
    }
    tags! {
        INPUT_TRANSFORM "input-transform" Medium "Camera-native data to scene-linear color RGB, as an ACES Input Transform.";
        LOOK "look" Low "Technical or creative adjustment of scene-referred RGB, as an ACES Look Transform.";
        OUTPUT_TRANSFORM "output-transform" Low "Scene-referred to display-referred rendering, as an ACES Output Transform.";
        IO "io" High "Reads or writes image files.";
        BAYER_RAW "bayer-raw" Medium "Works on Bayer sensor data before demosaicing.";
        DEMOSAIC "demosaic" Medium "Reconstructs full RGB from a color filter array.";
        RECOVERY "recovery" Medium "Restores what the capture lost or distorted, such as clipped highlights.";
        GEOMETRY "geometry" Medium "Changes pixel positions or sampling.";
        COLOR "color" Medium "Converts color spaces or white points.";
        TONEMAP "tonemap" Low "Compresses scene range into display range.";
        VIEW "view" Low "Shows an image in the GUI.";
        SCOPE "scope" Low "Measures an image for display.";
    }
}
pub use tag::TAGS;

/// Every processing node kind, in pipeline order. A kind missing here cannot
/// be added or loaded from a project.
pub static NODES: &[&Kind] = &[
    &raw::read::KIND,
    &hot_pixels::remove::KIND,
    &reduce::bayer::compute::KIND,
    &bayer_gains::compute::KIND,
    &highlights::reconstruct::KIND,
    &bayer_2x2::compute::KIND,
    &rcd::compute::KIND,
    &lens::correct::KIND,
    &camera_to_rgb::compute::KIND,
    &chromatic_adaptation::compute::KIND,
    &orientation::compute::KIND,
    &perspective::transform::KIND,
    &resample::compute::KIND,
    &reduce::rgb::compute::KIND,
    &exposure::compute::KIND,
    &sharpen::compute::KIND,
    &pyramid::expand::KIND,
    &sigmoid::apply::KIND,
    &export::tiff::KIND,
    &export::jpeg::write::KIND,
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
