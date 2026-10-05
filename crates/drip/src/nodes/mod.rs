//! The built-in node kinds.

mod bin2x2;
mod camera;
mod demosaic;
pub use demosaic::RCD;
mod export;
mod exposure;
pub use exposure::EXPOSURE;
mod highlights;
mod histogram;
mod scope_settings;
pub use highlights::HIGHLIGHTS;
mod preview;
mod raw;
pub mod scopes;
pub use scopes::{VECTORSCOPE, WAVEFORM};
pub mod sigmoid;
mod white_balance;

pub use bin2x2::BIN_2X2;
pub use camera::CAMERA_TO_REC2020;
pub use export::TIFF;
pub use histogram::HISTOGRAM;
pub use preview::PREVIEW;
pub use raw::{READ, downsample, normalize};
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
