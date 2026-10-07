//! Image preview preparation; display output remains in the renderer.

use std::sync::Arc;

use drip::image::{Rec2020Mat, Rec2020Rgb, Rgb};
use drip::node::{EvalContext, KernelError};
use drip::nodes::preview::{Mode, Preview};
use drip::ports::MatRef;
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
    (image,): (MatRef<'_, 3, dyn Rec2020Rgb>,),
    ctx: &EvalContext<'_>,
) -> Result<PreviewImage, KernelError> {
    let input = image.rgb();
    let output = || profile::Output::load(&p.output, ctx.resources());
    let pixels = match p.mode {
        Mode::None => None,
        Mode::Softproof => Some(softproof(&output()?, &input.pixels)?),
        Mode::Gamutcheck => Some(gamutcheck(&output()?, &input.pixels)?),
    };
    let mut view = match pixels {
        None => PreviewImage::from_input(&image),
        Some(pixels) => {
            let proof = Rec2020Mat::from(Arc::new(Rgb { pixels, ..**input }));
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
