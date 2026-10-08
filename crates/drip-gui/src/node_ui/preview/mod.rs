//! Image preview preparation; display output remains in the renderer.

use std::sync::Arc;

use drip::image::{ColorImage, Gpu, Rgb};
use drip::node::{EvalContext, KernelError};
use drip::nodes::preview::{Mode, Preview};
use drip::profile;

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
    (image,): (&ColorImage<Gpu>,),
    ctx: &EvalContext<'_>,
) -> Result<PreviewImage, KernelError> {
    image.require_additive_color()?;
    ctx.compute()?.check_buffer(image.gpu_buffer())?;
    let mut view = match p.mode {
        Mode::None => PreviewImage::resident(image),
        Mode::Softproof | Mode::Gamutcheck => {
            let output = profile::Output::load(&p.output, ctx.resources())?;
            // LittleCMS owns the proofing algorithm; this explicit host boundary
            // is limited to the selected proof mode, never the normal preview.
            let host = image.download(ctx.compute()?)?;
            let pixels = match p.mode {
                Mode::Softproof => softproof(&output, &host.pixels)?,
                Mode::Gamutcheck => gamutcheck(&output, &host.pixels)?,
                Mode::None => unreachable!(),
            };
            let proof = ColorImage::try_new(
                Arc::new(Rgb { pixels, ..**host.rgb() }),
                image.interpretation().after_rendering(),
            )?;
            PreviewImage::new(&proof)
        }
    };
    view.interpolation = p.interpolation;
    Ok(view)
}

struct PreviewGui;
#[drip_macros::gui_node]
impl super::GuiNode for PreviewGui {
    type Node = drip::nodes::preview::PreviewNode;
    type Presentation = PreviewImage;
    const NODE: &'static drip::node::TypedNode<Self::Node> = &drip::nodes::PREVIEW;
    const PREPARE: Option<super::Prepare<Self>> = Some(prepare);
}
