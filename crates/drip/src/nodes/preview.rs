//! Preview declaration; preparation belongs to the frontend.

use crate::node::KernelError;
use crate::{
    image::{ColorImage, Gpu},
    param::ParamKind,
    profile,
};

#[derive(Clone, Copy, crate::Choice)]
pub enum Mode {
    #[choice("none")]
    None,
    #[choice("softproof")]
    Softproof,
    #[choice("gamutcheck")]
    Gamutcheck,
}

/// Shows a Rec.2020 image; the frontend handles the display transform.
#[derive(crate::Parameters)]
pub struct Preview {
    #[param(ParamKind::Bool { default: false })]
    pub interpolation: bool,
    #[param(Mode::None.schema())]
    pub mode: Mode,
    #[param(flatten)]
    pub output: profile::Settings,
}

/// Preview a GPU ColorImage in additive Rec.2020/D65 coordinates.
///
/// The frontend must check this coordinate refinement before its display transform;
/// original-scene proportionality is not required. Normal drawing stays resident;
/// CPU profile proofing requests an explicit readback. Preview does not tone-map.
///
/// Softproof simulates the selected profile and intent. Gamutcheck highlights possible
/// gamut clipping in cyan. Interpolation selects bilinear display sampling; off
/// preserves discrete pixels. Produces an image preview view.
#[crate::node(kind = PREVIEW, id = "view.preview", category = "view", name = "Preview", outputs = [])]
fn preview(#[params] p: Preview, image: &ColorImage<Gpu>) -> Result<(), KernelError>;
