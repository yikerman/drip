//! Exposure multiplication preserves the input RGB interpretation.

use crate::image::ScaleInvariant;
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;
use crate::ports::{MatRef, Preserved};
use rayon::prelude::*;

#[derive(crate::Parameters)]
pub struct Exposure {
    /// Multiplication by 2^ev in the input's linear RGB coordinates.
    #[param(ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 })]
    ev: f32,
}

/// Multiply linear RGB by 2^ev: x * 2^ev.
///
/// Requires linear RGB whose full interpretation permits positive scaling.
/// The output keeps the input's type and is pending until the input has one.
#[crate::node(kind = EXPOSURE, id = "color.exposure", category = "color", name = "Exposure", outputs = ["image"])]
fn exposure(
    p: Exposure,
    (image,): (MatRef<'_, 3, dyn ScaleInvariant>,),
    _: &EvalContext<'_>,
) -> Result<(Preserved<0>,), KernelError> {
    let gain = p.ev.exp2();
    let pixels = image.rgb().pixels.par_iter().map(|p| p.map(|v| v * gain)).collect();
    Ok((image.preserve::<0>(pixels),))
}
