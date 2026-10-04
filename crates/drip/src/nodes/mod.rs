//! The built-in node kinds.

mod color;
mod raw;

pub use color::{BIN_2X2, CAMERA_TO_REC2020, WHITE_BALANCE};
pub use raw::{READ, normalize};

use crate::node::Registry;

pub fn registry() -> Registry {
    [&READ, &WHITE_BALANCE, &BIN_2X2, &CAMERA_TO_REC2020]
        .into_iter()
        .fold(Registry::default(), Registry::with)
}
