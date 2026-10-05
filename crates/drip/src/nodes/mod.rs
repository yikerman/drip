//! The built-in node kinds.

mod color;
mod export;
mod raw;
mod tone;
mod view;

pub use color::{BIN_2X2, CAMERA_TO_REC2020, WHITE_BALANCE};
pub use export::TIFF;
pub use raw::{READ, normalize};
pub use tone::SIGMOID;
pub use view::{HISTOGRAM, PREVIEW};

use crate::node::Registry;

pub fn registry() -> Registry {
    [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020, &SIGMOID, &PREVIEW, &HISTOGRAM, &TIFF]
        .into_iter()
        .fold(Registry::default(), Registry::with)
}
