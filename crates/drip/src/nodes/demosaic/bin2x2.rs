//! Bayer-cell averaging with explicit sensor phase and preview spacing.

use crate::image::{CameraRgb, Gpu, Mosaic};
use crate::node::{EvalContext, KernelError};
use std::sync::Arc;

/// Average each 2 × 2 Bayer cell into one CameraRgb pixel.
///
/// Requires one red, two green and one blue site per cell. The two greens are
/// averaged after any upstream channel balancing. Output sample spacing doubles;
/// partial cells are cropped. This changes noise statistics but retains camera
/// response coordinates, without assigning standard colorimetry.
#[crate::node(kind = BIN_2X2, id = "demosaic.bin2x2", category = "demosaic", name = "Debayer", outputs = ["image"])]
fn bin2x2(
    mosaic: &Mosaic<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<CameraRgb<Gpu>>,), KernelError> {
    let m = super::preview(mosaic, ctx)?;
    super::super::gpu::require_bayer(&m)?;
    let (width, height) = (m.width() / 2, m.height() / 2);
    if width == 0 || height == 0 {
        return Err(KernelError::Failed("Bayer averaging needs a complete 2 × 2 cell".into()));
    }
    let scale = m
        .scale()
        .checked_mul(2)
        .ok_or_else(|| KernelError::Failed("sample scale overflow".into()))?;
    let mut parameters = vec![m.width() as f32, m.height() as f32, 2.0];
    parameters
        .extend(m.interpretation().cfa.colors.iter().map(|&c| if c == 3 { 1.0 } else { c as f32 }));
    let output = super::super::gpu::pointwise(
        ctx.compute()?,
        "bin2x2",
        m.gpu_buffer(),
        width * height * 3,
        width * height,
        &parameters,
    )?;
    Ok((Arc::new(CameraRgb::from_gpu(
        output,
        width,
        height,
        scale,
        (*m.interpretation().camera).clone(),
    )?),))
}
