//! Channel gain correction in sensor response coordinates.

use crate::image::{Gpu, Mosaic};
use crate::node::{EvalContext, KernelError};
use std::sync::Arc;

/// Multiply Mosaic samples by the capture's as-shot gains.
///
/// Requires a valid color-filter pattern and finite positive channel gains.
/// Each channel's saturation threshold scales with its samples. Output remains
/// sensor response data; the operation does not establish standard colorimetry.
/// Apply once when using these as-shot gains as the camera conversion convention.
#[crate::node(kind = WHITE_BALANCE, id = "color.white_balance", category = "color", name = "White balance", outputs = ["mosaic"])]
fn white_balance(
    mosaic: &Mosaic<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<Mosaic<Gpu>>,), KernelError> {
    let meaning = mosaic.interpretation();
    let gains = meaning.camera.white_balance;
    let mut parameters = vec![mosaic.width() as f32, meaning.cfa.size as f32];
    parameters.extend(meaning.cfa.colors.iter().map(|&c| gains[c as usize]));
    let output = super::gpu::pointwise(
        ctx.compute()?,
        "white_balance",
        mosaic.gpu_buffer(),
        mosaic.gpu_buffer().len(),
        mosaic.gpu_buffer().len(),
        &parameters,
    )?;
    let mut meaning = meaning.clone();
    meaning.white = std::array::from_fn(|c| meaning.white[c] * gains[c]);
    Ok((Arc::new(Mosaic::from_gpu(
        output,
        mosaic.width(),
        mosaic.height(),
        mosaic.scale(),
        meaning,
    )?),))
}
