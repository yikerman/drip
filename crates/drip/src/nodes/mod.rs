//! The built-in node kinds.

mod camera;
mod demosaic;
mod export;
mod exposure;
mod highlights;
pub mod preview;
mod raw;
pub mod scopes;
pub mod sigmoid;
mod white_balance;

pub use camera::CAMERA_TO_REC2020;
pub use demosaic::{BIN_2X2, RCD, downsample};
pub use export::TIFF;
pub use exposure::EXPOSURE;
pub use highlights::HIGHLIGHTS;
pub use preview::PREVIEW;
pub use raw::{READ, normalize};
pub use scopes::{HISTOGRAM, VECTORSCOPE, WAVEFORM};
pub use sigmoid::SIGMOID;
pub use white_balance::WHITE_BALANCE;

use crate::node::Registry;

pub fn registry() -> Registry {
    crate::node::NODE_KINDS.iter().copied().fold(Registry::default(), Registry::with)
}
