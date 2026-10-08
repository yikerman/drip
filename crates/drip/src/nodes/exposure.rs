//! Uniform exposure multiplication in additive color coordinates.

use crate::image::{ColorImage, Gpu};
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;
use std::sync::Arc;

#[derive(crate::Parameters)]
pub struct Exposure {
    /// Stops: multiply the represented light by 2^ev.
    #[param(ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 })]
    ev: f32,
}

/// Multiply ColorImage samples by 2^ev in additive color coordinates.
///
/// Requires additive coordinates, not a claim of original scene accuracy. The
/// output retains color coordinates and scales an existing scene estimate; after
/// rendering it scales the represented light without restoring scene provenance.
#[crate::node(kind = EXPOSURE, id = "color.exposure", category = "color", name = "Exposure", outputs = ["image"])]
fn exposure(
    #[params] p: Exposure,
    image: &ColorImage<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<ColorImage<Gpu>>,), KernelError> {
    let image = image.require_additive_color()?;
    let gain = p.ev.exp2();
    let meaning = image.interpretation().after_exposure(gain)?;
    let output = super::gpu::pointwise(
        ctx.compute()?,
        "exposure",
        image.gpu_buffer(),
        image.gpu_buffer().len(),
        image.gpu_buffer().len(),
        &[gain],
    )?;
    Ok((Arc::new(ColorImage::from_gpu(
        output,
        image.width(),
        image.height(),
        image.scale(),
        meaning,
    )?),))
}
