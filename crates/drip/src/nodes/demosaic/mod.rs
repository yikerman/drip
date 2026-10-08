//! Sensor-plane preview reduction followed by camera RGB reconstruction.

use crate::compute::Compute;
use crate::image::{CameraRgb, Gpu, Mosaic};
use crate::node::{EvalContext, KernelError};
use std::sync::Arc;
mod bin2x2;
mod rcd;

pub use bin2x2::BIN_2X2;

/// Reconstruct CameraRgb from a Bayer Mosaic using Ratio Corrected Demosaicing.
///
/// Requires a 2 × 2 Bayer pattern with matching sample origin. Preview reduction
/// averages equal sensor phases before interpolation. The output estimates three
/// camera responses at each site; interpolation changes their noise statistics
/// and does not establish XYZ colorimetry. The outer ten pixels use bilinear
/// interpolation while retaining measured samples.
#[crate::node(kind = RCD, id = "demosaic.rcd", category = "demosaic", name = "Demosaic", outputs = ["image"], references = [("RCD algorithm", "https://github.com/LuisSR/RCD-Demosaicing")])]
fn rcd(
    mosaic: &Mosaic<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<CameraRgb<Gpu>>,), KernelError> {
    let mosaic = preview(mosaic, ctx)?;
    let output = rcd::process(ctx.compute()?, &mosaic)?;
    Ok((Arc::new(CameraRgb::from_gpu(
        output,
        mosaic.width(),
        mosaic.height(),
        mosaic.scale(),
        (*mosaic.interpretation().camera).clone(),
    )?),))
}

pub(super) fn preview(
    input: &Mosaic<Gpu>,
    ctx: &EvalContext<'_>,
) -> Result<Mosaic<Gpu>, KernelError> {
    let mut mosaic = input.clone();
    while mosaic.scale() < ctx.scale() {
        // The smallest valid Bayer image is one complete cell. Further preview
        // reduction cannot retain its sensor-phase contract.
        if mosaic.width() < 4 || mosaic.height() < 4 {
            break;
        }
        mosaic = downsample(ctx.compute()?, &mosaic)?;
    }
    Ok(mosaic)
}

/// Halve a Bayer Mosaic by averaging four samples of each sensor phase.
///
/// Only whole 4 × 4 input cells contribute. Output spacing doubles; the CFA
/// origin and channel normalization are retained. A smaller input is rejected
/// because it cannot produce a complete output Bayer cell.
pub fn downsample(compute: &Compute, m: &Mosaic<Gpu>) -> Result<Mosaic<Gpu>, KernelError> {
    super::gpu::require_bayer(m)?;
    let (width, height) = (m.width() / 4 * 2, m.height() / 4 * 2);
    if width == 0 || height == 0 {
        return Err(KernelError::Failed(
            "preview reduction needs a complete 4 × 4 Bayer block".into(),
        ));
    }
    let scale = m
        .scale()
        .checked_mul(2)
        .ok_or_else(|| KernelError::Failed("sample scale overflow".into()))?;
    let output = super::gpu::pointwise(
        compute,
        "downsample",
        m.gpu_buffer(),
        width * height,
        width * height,
        &[m.width() as f32, m.height() as f32],
    )?;
    Ok(Mosaic::from_gpu(output, width, height, scale, m.interpretation().clone())?)
}
