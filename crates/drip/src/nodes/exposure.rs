//! Exposure multiplication preserves the input RGB interpretation.

use crate::image::{Rec2020Mat, Rgb};
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;
use rayon::prelude::*;
use std::sync::Arc;

#[derive(crate::Parameters)]
pub struct Exposure {
    /// Multiplication by 2^ev in the input's linear RGB coordinates.
    #[param(ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 })]
    ev: f32,
}

/// Multiply linear RGB by 2^ev: x * 2^ev.
///
/// Input and output use linear Rec.2020 coordinates. After creative tone mapping,
/// this scales the rendered light rather than restoring scene exposure.
#[crate::node(kind = EXPOSURE, id = "color.exposure", category = "color", name = "Exposure", outputs = ["image"])]
fn exposure(
    p: Exposure,
    (image,): (&Rec2020Mat,),
    _: &EvalContext<'_>,
) -> Result<(Arc<Rec2020Mat>,), KernelError> {
    let gain = p.ev.exp2();
    let pixels = image.rgb().pixels.par_iter().map(|p| p.map(|v| v * gain)).collect();
    Ok((Arc::new(Rec2020Mat::from(Arc::new(Rgb { pixels, ..**image.rgb() }))),))
}
