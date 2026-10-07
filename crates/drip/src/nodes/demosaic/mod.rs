//! Bayer demosaicing with RCD or cell averaging. Preview reduction belongs before
//! interpolation, after sensor-space processing such as highlight reconstruction.

use crate::image::{CameraRgb, Mosaic, Rgb};
use crate::node::{EvalContext, KernelError};
use std::sync::Arc;
mod bin2x2;
mod rcd;

pub use bin2x2::BIN_2X2;

/// Bayer demosaicing with Ratio Corrected Demosaicing (RCD) and a bilinear border.
///
/// Preview reduction precedes interpolation. Produces camera RGB.
#[crate::node(kind = RCD, id = "demosaic.rcd", category = "demosaic", name = "Demosaic", outputs = ["image"], references = [("RCD algorithm", "https://github.com/LuisSR/RCD-Demosaicing")])]
fn rcd(
    _: (),
    (mosaic,): (&Mosaic,),
    ctx: &EvalContext<'_>,
) -> Result<(Arc<CameraRgb>,), KernelError> {
    let mosaic = preview(mosaic, ctx);
    let pixels = rcd::process(&mosaic);
    let image = Rgb { width: mosaic.width, height: mosaic.height, scale: mosaic.scale, pixels };
    Ok((Arc::new(CameraRgb {
        data: Arc::new(image),
        interpretation: mosaic.interpretation().camera.clone(),
    }),))
}

/// Borrow full detail, allocate only when a lower-resolution mosaic is needed.
pub(super) fn preview<'a>(input: &'a Mosaic, ctx: &EvalContext) -> std::borrow::Cow<'a, Mosaic> {
    let mut mosaic = std::borrow::Cow::Borrowed(input);
    while mosaic.scale < ctx.scale() {
        mosaic = std::borrow::Cow::Owned(downsample(&mosaic));
    }
    mosaic
}

/// Halves a Bayer mosaic by averaging four sites of each phase. Only whole
/// 4 × 4 input cells contribute, so every output remains a complete Bayer cell.
pub fn downsample(m: &Mosaic) -> Mosaic {
    let (width, height) = (m.width / 4 * 2, m.height / 4 * 2);
    let mut data = Vec::with_capacity(width * height);
    for row in 0..height {
        for col in 0..width {
            let (r, c) = (row / 2 * 4 + row % 2, col / 2 * 4 + col % 2);
            let i = r * m.width + c;
            data.push(
                (m.samples()[i]
                    + m.samples()[i + 2]
                    + m.samples()[i + 2 * m.width]
                    + m.samples()[i + 2 * m.width + 2])
                    * 0.25,
            );
        }
    }
    m.with_buffer(Arc::new(crate::image::RawMat::from_samples(width, height, m.scale * 2, data)))
}
