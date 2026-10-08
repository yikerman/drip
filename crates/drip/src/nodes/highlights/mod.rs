//! Reconstruction of clipped sensor responses before spatial reduction.

use crate::image::{Gpu, Mosaic};
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;
use std::sync::Arc;
mod opposed;

#[derive(crate::Parameters)]
pub struct Highlights {
    /// Fraction of the current per-channel saturation threshold.
    #[param(ParamKind::Float { min: 0.5, max: 1.0, default: 0.98 })]
    threshold: f32,
}

/// Estimate clipped Mosaic samples using inpaint opposed.
///
/// Requires a Bayer mosaic and saturation levels in its current sample scale.
/// As-shot-balanced channels are the estimator's intended domain: it uses the
/// cube-root mean of the other two channels plus chrominance learned nearby.
/// Unclipped samples remain unchanged; reconstructed values are estimates, not
/// recovered measurements suitable for chart calibration. Threshold metadata
/// remains the sensor saturation reference, not a bound on output values.
#[crate::node(kind = HIGHLIGHTS, id = "raw.highlights", category = "raw", name = "Highlights", outputs = ["mosaic"], references = [("darktable: inpaint opposed", "https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/highlight-reconstruction/")])]
fn highlights(
    #[params] p: Highlights,
    mosaic: &Mosaic<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<Mosaic<Gpu>>,), KernelError> {
    let output = opposed::process(ctx.compute()?, mosaic, p.threshold)?;
    Ok((Arc::new(Mosaic::from_gpu(
        output,
        mosaic.width(),
        mosaic.height(),
        mosaic.scale(),
        mosaic.interpretation().clone(),
    )?),))
}
