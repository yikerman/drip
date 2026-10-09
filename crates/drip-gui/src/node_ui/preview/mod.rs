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
    let output = || profile::Output::load(&p.output, &ctx.resources);
    let pixels = match p.mode {
        Mode::None => None,
        Mode::Softproof => Some(softproof(&output()?, &input.pixels)?),
        Mode::Gamutcheck => Some(gamutcheck(&output()?, &input.pixels)?),
    };
    let mut view = match pixels {
        None => PreviewImage::new(image),
        Some(pixels) => {
            let proof = Arc::new(Rgb { pixels: pixels.into(), ..(**input).clone() });
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
#[derive(Clone, Copy, Default, drip::Choice)]
pub enum Mode {
    #[default]
    #[choice("none")]
    None,
    #[choice("softproof")]
    Softproof,
    #[choice("gamutcheck")]
    Gamutcheck,
}
#[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    #[param(ParamKind::Bool { default: false })]
    pub interpolation: bool,
    #[param(Mode::None.schema())]
    pub mode: Mode,
    #[param(flatten)]
    #[serde(flatten)]
    pub output: profile::Settings,
}
fn preview_contract(
    _: &drip::runtime::GlobalContext,
    _: &Preview,
    image: Option<&ImageDesc<Color>>,
) -> drip::Result<()> {
    super::contracts::working(image)
}
/// Preview identity Rec.2020 ColorRgb. Softproof simulates the output profile;
/// gamutcheck marks clipping in cyan. Interpolation controls display sampling.
#[drip::node(id="view.preview", name="Preview", category="view", contract=preview_contract)]
fn observe(_: &KernelContext<'_>, _: &Preview, image: Read<'_, Cpu<ColorRgb>>) -> drip::Result<()> {
    let _ = image;
    Ok(())
}
