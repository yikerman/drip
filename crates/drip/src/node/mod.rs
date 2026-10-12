//! Processing nodes and their shared image-domain support.
pub mod bayer_2x2;
pub mod bayer_gains;
pub mod camera;
pub mod camera_to_rgb;
pub mod chromatic_adaptation;
pub mod color;
pub mod data;
pub mod export;
pub mod exposure;
pub mod highlights;
pub mod orientation;
pub mod profile;
pub mod raw;
pub mod rcd;
pub mod reduce;
mod shared_kernel;
pub mod sigmoid;
pub mod source;
pub mod templates;
pub use source::source;

/// Every processing node kind, in pipeline order. A kind missing here cannot
/// be added or loaded from a project.
pub static NODES: &[&crate::definition::Kind] = &[
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
