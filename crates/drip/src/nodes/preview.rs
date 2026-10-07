//! Preview declaration; preparation belongs to the frontend.

use crate::node::{EvalContext, KernelError};
use crate::ports::MatRef;
use crate::{image::Rec2020Rgb, param::ParamKind, profile};

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

/// Preview Rec.2020 RGB as linear light, without tone mapping.
///
/// Softproof simulates the selected profile and intent. Gamutcheck highlights possible
/// gamut clipping in cyan. Interpolation selects bilinear display sampling; off
/// preserves discrete pixels. Produces an image preview view.
#[crate::node(kind = PREVIEW, id = "view.preview", category = "view", name = "Preview", outputs = [])]
fn preview(
    p: Preview,
    (image,): (MatRef<'_, 3, dyn Rec2020Rgb>,),
    ctx: &EvalContext<'_>,
) -> Result<(), KernelError>;
