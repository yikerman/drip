//! Image preview preparation; display output remains in the renderer.

use std::sync::Arc;

use crate::node_ui::data::{PrepareContext, Rgb};
use drip::Error as KernelError;
use drip::node::profile;

use proofing::{gamutcheck, softproof};
#[cfg(test)]
pub use view::ImageView;
pub use view::PreviewImage;

#[cfg(test)]
mod export_tests;
mod proofing;
#[cfg(test)]
mod tests;
mod view;

pub fn prepare(
    p: Preview,
    image: &Arc<Rgb>,
    ctx: &PrepareContext,
) -> Result<PreviewImage, KernelError> {
    let input = image;
    let pixels = match p.mode {
        Mode::None => None,
        Mode::Softproof(output) => {
            Some(softproof(&profile::Output::load(&output, &ctx.resources)?, &input.pixels)?)
        }
        Mode::Gamutcheck(output) => {
            Some(gamutcheck(&profile::Output::load(&output, &ctx.resources)?, &input.pixels)?)
        }
    };
    let mut view = match pixels {
        None => PreviewImage::new(image),
        Some(pixels) => {
            let proof =
                Arc::new(Rgb { pixels: std::sync::Arc::new(pixels.into()), ..(**input).clone() });
            PreviewImage::new(&proof)
        }
    };
    view.interpolation = p.interpolation;
    Ok(view)
}

struct PreviewGui;
#[drip_macros::gui_node]
impl super::GuiNode for PreviewGui {
    type Parameters = Preview;
    type Presentation = PreviewImage;
    const ID: &'static str = "view.preview";
    fn parameter_ui(_: &Preview, path: &str) -> super::ParameterUi {
        super::profile::parameter_ui(path)
    }
    const PREPARE: Option<super::Prepare<Self>> =
        Some(|p, inputs, ctx| prepare(p, &ctx.image::<drip::node::data::Color>(inputs)?, ctx));
}

use drip::{
    node::data::{Color, ColorRgb, ImageDesc},
    param::ParamKind,
    ports::{Cpu, Read},
    runtime::KernelContext,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, drip::Choice)]
pub enum Mode {
    #[default]
    #[choice("none")]
    None,
    #[choice("softproof")]
    Softproof(profile::Settings),
    #[choice("gamutcheck")]
    #[label("Gamut check")]
    Gamutcheck(profile::Settings),
}
#[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    /// Use bilinear display sampling when enabled, nearest-neighbor sampling otherwise.
    #[param(ParamKind::Bool { default: false })]
    pub interpolation: bool,
    /// Show the image directly, simulate the output profile, or mark out-of-gamut colors in cyan.
    #[label("Proof mode")]
    #[param(Mode::None.schema())]
    pub mode: Mode,
}
fn preview_contract(
    _: &drip::runtime::GlobalContext,
    _: &Preview,
    image: Option<&ImageDesc<Color>>,
) -> drip::Result<()> {
    super::contracts::working(image)
}
/// Display identity-encoded Rec.2020/D65 ColorRgb with relative white 1.
/// Softproof simulates an output profile; Gamut check marks out-of-gamut colors in
/// cyan. Each proof mode owns its output settings. Interpolation selects bilinear
/// or nearest-neighbor display sampling and does not alter processing pixels.
#[drip::node(id="view.preview", name="Preview", category="View", contract=preview_contract)]
fn observe(_: &KernelContext<'_>, _: &Preview, image: Read<'_, Cpu<ColorRgb>>) -> drip::Result<()> {
    let _ = image;
    Ok(())
}
