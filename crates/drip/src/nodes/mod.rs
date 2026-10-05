//! The built-in node kinds.

mod bin2x2;
mod camera;
mod export;
mod histogram;
mod preview;
mod raw;
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
    [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020, &SIGMOID, &PREVIEW, &HISTOGRAM, &TIFF]
        .into_iter()
        .fold(Registry::default(), Registry::with)
}

fn single(output: crate::value::Value) -> Result<crate::node::Evaluated, String> {
    Ok(crate::node::Evaluated { outputs: vec![output], view: None })
}
