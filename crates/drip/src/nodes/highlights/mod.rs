//! Sensor-space highlight repair before preview reduction and demosaicing.

use crate::image::Mosaic;
use crate::node::{EvalContext, Evaluated, KernelError};
use crate::param::ParamKind;
use std::sync::Arc;
mod opposed;

#[derive(crate::Parameters)]
pub struct Highlights {
    /// Fraction of the per-channel saturation threshold used for reconstruction.
    #[param(ParamKind::Float { min: 0.5, max: 1.0, default: 0.98 })]
    threshold: f32,
}

/// Reconstruct clipped Bayer samples with inpaint opposed.
///
/// Assumes white-balanced input. Threshold is a fraction of each channel's saturation
/// level. Runs before preview reduction.
#[crate::node(kind = HIGHLIGHTS, id = "raw.highlights", category = "raw", name = "Highlights", outputs = ["mosaic"], references = [("darktable: inpaint opposed", "https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/highlight-reconstruction/")])]
fn highlights(
    p: Highlights,
    (mosaic,): (&Mosaic,),
    _: &EvalContext<'_>,
) -> Result<Evaluated<(Arc<Mosaic>,)>, KernelError> {
    let data = opposed::process(mosaic, p.threshold);
    let data = crate::image::RawMat::from_samples(mosaic.width, mosaic.height, mosaic.scale, data);
    Ok(Evaluated::new((Arc::new(mosaic.with_buffer(Arc::new(data))),)))
}
