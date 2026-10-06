//! The built-in node kinds.

mod camera;
mod demosaic;
mod export;
mod exposure;
mod highlights;
mod preview;
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
    [
        &READ,
        &WHITE_BALANCE,
        &HIGHLIGHTS,
        &BIN_2X2,
        &RCD,
        &CAMERA_TO_REC2020,
        &EXPOSURE,
        &SIGMOID,
        &PREVIEW,
        &HISTOGRAM,
        &WAVEFORM,
        &VECTORSCOPE,
        &TIFF,
    ]
    .into_iter()
    .fold(Registry::default(), Registry::with)
}
