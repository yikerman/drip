// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2020-2026 darktable developers.
//! Per-channel sigmoid and hue/energy correction adapted from darktable \[1\].
//! Fixed Rec.2020 primaries use its smooth preset's attenuation and rotation;
//! purity recovery is zero. Drip retains its 0.18 grey point and zero black.
//! Coefficients use analytic slopes and the curve uses log space to avoid
//! overflowing powers; neither changes the underlying log-logistic curve.
//!
//! \[1\] darktable developers, “sigmoid.c” and “custom_primaries.c,” commit
//! 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3, 2026. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src>
//! License: GPL-3.0-or-later; see THIRD_PARTY.md for credits and the upstream license.

use crate::node::color::{D65, REC2020};
use crate::{
    Error, Result,
    node::color,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Contrast at the fixed 0.18 grey point.
    #[param(crate::param::ParamKind::Float { min:0.5, max:4.0, default:1.5 })]
    pub contrast: f32,
    /// Asymmetry of the curve around grey.
    #[param(crate::param::ParamKind::Float { min:-1.0, max:1.0, default:-0.2 })]
    pub skew: f32,
    /// Strength of the hue and energy correction.
    #[param(crate::param::ParamKind::Float { min:0.0, max:1.0, default:0.0 })]
    pub preserve_hue: f32,
}
impl Settings {
    /// Sample the processing curve for controls, using the same kernel function.
    pub fn curve(&self) -> impl Fn(f32) -> f32 {
        let coefficients = prepare_values(self);
        move |value| kernel::curve(value, &coefficients, 0.0)
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self { contrast: 1.5, skew: -0.2, preserve_hue: 0.0 }
    }
}
fn validate(p: &Settings) -> Result<()> {
    if !p.contrast.is_finite()
        || p.contrast <= 0.0
        || !(-1.0..=1.0).contains(&p.skew)
        || !(0.0..=1.0).contains(&p.preserve_hue)
    {
        return Err(Error::Contract(
            "sigmoid needs positive contrast, skew in [-1,1], hue preservation in [0,1]".into(),
        ));
    }
    let c = prepare_values(p);
    if c.iter().any(|v| !v.is_finite()) {
        return Err(Error::Contract("sigmoid coefficients are not representable".into()));
    }
    Ok(())
}
fn prepare_values(p: &Settings) -> [f32; 22] {
    let grey = f64::from(0.18f32);
    let paper = 5.0f64.powf(-f64::from(p.skew));
    let grey_root = grey.powf(1.0 / paper);
    // Match the unskewed curve's derivative at grey: c * (1-grey).
    let film = f64::from(p.contrast) * (1.0 - grey) / (paper * (1.0 - grey_root));
    let log_exposure = film * grey.ln() + (1.0 / grey_root - 1.0).ln();
    let base = color::rgb_to_xyz(REC2020, D65);
    let to_base = color::inverse(&base);
    let inset = color::mul(&to_base, &color::rgb_to_xyz(primaries([0.9, 0.9, 0.85]), D65));
    let outset =
        color::inverse(&color::mul(&to_base, &color::rgb_to_xyz(primaries([1.0; 3]), D65)));
    let mut result = [0.; 22];
    result[..4].copy_from_slice(&[film as f32, paper as f32, log_exposure as f32, p.preserve_hue]);
    result[4..13].copy_from_slice(color::to_f32(&inset).as_flattened());
    result[13..].copy_from_slice(color::to_f32(&outset).as_flattened());
    result
}

fn apply_contract(
    _: &GlobalContext,
    p: &Settings,
    image: Option<&ImageDesc<Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    validate(p)?;
    if image.is_some_and(|d| d.interpretation != super::raw::working_color()) {
        return Err(Error::Contract(
            "sigmoid requires identity-encoded Rec.2020/D65 with relative white 1".into(),
        ));
    }
    Ok((image.cloned(),))
}
/// Apply a sigmoid contrast curve and hue correction to identity-encoded
/// Rec.2020/D65 ColorRgb with relative white 1. Grey is fixed at 0.18; black
/// stays zero and the curve approaches one. Outputs edited ColorRgb in the same
/// coordinates. Negative channels use an achromatic projection; this does not
/// apply sRGB encoding.
#[crate::node(id="tone.sigmoid",
name="Sigmoid",
category="Tone",
contract=apply_contract,
references=[("darktable sigmoid", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/sigmoid.c")])]
pub fn apply(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<ColorRgb>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    // These constants belong to this invocation, not to a graph payload.
    let coefficients = DeviceBuffer::<()>::new(
        ctx.client()?.create(cubecl::bytes::Bytes::from_elems(prepare_values(p).to_vec())),
    );
    let (count, dim) = crate::runtime::dispatch_dims(image.desc.extent.pixels());
    kernel::sigmoid::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        coefficients.argument(),
        output.data.argument(),
    );
    Ok(())
}

/// Rotate each spectral direction to the base triangle's edge, then attenuate.
/// This follows darktable's ray/edge construction, not rotation about the origin.
fn primaries(scale: [f64; 3]) -> [[f64; 2]; 3] {
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    std::array::from_fn(|i| {
        let d = sub(REC2020[i], D65);
        let angle = d[1].atan2(d[0]) + [2.0f64, -1.0, -3.0][i].to_radians();
        let dir = [angle.cos(), angle.sin()];
        let distance = (0..3)
            .filter_map(|j| {
                let edge = sub(REC2020[(j + 1) % 3], REC2020[j]);
                let t = cross(sub(REC2020[j], D65), edge) / cross(dir, edge);
                (t >= 0.0).then_some(t)
            })
            .fold(f64::INFINITY, f64::min);
        [D65[0] + scale[i] * distance * dir[0], D65[1] + scale[i] * distance * dir[1]]
    })
}

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use crate::node::shared_kernel::down64;
    use cubecl::prelude::*;
    #[cube]
    pub(crate) fn curve(value: f32, p: &[f32], log_scale: f32) -> f32 {
        if value <= 0.0f32 {
            0.0f32
        } else {
            let z = p[2] - p[0] * (value.ln() + log_scale);
            let softplus = z.max(0.0f32) + (1.0f32 + (-z.abs()).exp()).ln();
            (-p[1] * softplus).exp()
        }
    }
    // CubeCL expands scalar assignments; std::mem::swap is outside its kernel DSL.
    #[allow(clippy::manual_swap)]
    #[cube(launch)]
    pub fn sigmoid(input: &[f32], p: &[f32], output: &mut [f32]) {
        let i = ABSOLUTE_POS * 3;
        if i < input.len() {
            let mut rgb = Array::<f32>::new(3usize);
            let mut work = Array::<f32>::new(3usize);
            let mut mapped = Array::<f32>::new(3usize);
            let magnitude = input[i].abs().max(input[i + 1].abs()).max(input[i + 2].abs());
            let input_scale = if magnitude > 1e20f32 { 18446744073709551616.0f32 } else { 1.0f32 };
            let mut bounded = Array::<f32>::new(3usize);
            for c in 0..3 {
                bounded[c] = if magnitude > 1e20f32 { down64(input[i + c]) } else { input[i + c] };
            }
            let log_scale = input_scale.ln();
            let average =
                (bounded[0] / 3.0f32 + bounded[1] / 3.0f32 + bounded[2] / 3.0f32).max(0.0f32);
            let minimum = bounded[0].min(bounded[1]).min(bounded[2]);
            // Scale before subtraction, retaining the f64 CPU ratio's overflow protection.
            let scale = average.max(-minimum);
            let saturation = if minimum < 0.0f32 {
                (average / scale) / (average / scale - minimum / scale)
            } else {
                1.0f32
            };
            for c in 0..3 {
                rgb[c] = ((1.0f32 - saturation) * average + saturation * bounded[c]).max(0.0f32);
            }
            for c in 0..3 {
                work[c] = p[4 + c * 3] * rgb[0] + p[5 + c * 3] * rgb[1] + p[6 + c * 3] * rgb[2];
                mapped[c] = curve(work[c], p, log_scale);
            }
            let mut lo = 0usize;
            let mut mid = 1usize;
            let mut hi = 2usize;
            if work[lo] > work[mid] {
                let t = lo;
                lo = mid;
                mid = t;
            }
            if work[mid] > work[hi] {
                let t = mid;
                mid = hi;
                hi = t;
            }
            if work[lo] > work[mid] {
                let t = lo;
                lo = mid;
                mid = t;
            }
            let chroma = work[hi] - work[lo];
            let fraction = if chroma != 0.0f32 { (work[mid] - work[lo]) / chroma } else { 0.0f32 };
            let hue = p[3];
            let corrected = mapped[lo] + (mapped[hi] - mapped[lo]) * fraction;
            let naive = (1.0f32 - hue) * mapped[mid] + hue * corrected;
            let scale2 = work[lo].max(work[mid]);
            let blend = if scale2 != 0.0f32 {
                2.0f32 * (work[lo] / scale2) / (work[lo] / scale2 + work[mid] / scale2)
            } else {
                0.0f32
            };
            let energy = blend * (mapped[0] + mapped[1] + mapped[2])
                + (1.0f32 - blend) * (mapped[lo] + naive + mapped[hi]);
            if naive <= mapped[mid] {
                mapped[mid] = ((1.0f32 - hue) * mapped[mid]
                    + hue * (fraction * mapped[hi] + (1.0f32 - fraction) * (energy - mapped[hi])))
                    / (1.0f32 + hue * (1.0f32 - fraction));
                mapped[lo] = energy - mapped[hi] - mapped[mid];
            } else {
                mapped[mid] = ((1.0f32 - hue) * mapped[mid]
                    + hue * (mapped[lo] * (1.0f32 - fraction) + fraction * (energy - mapped[lo])))
                    / (1.0f32 + hue * fraction);
                mapped[hi] = energy - mapped[lo] - mapped[mid];
            }
            for c in 0..3 {
                output[i + c] = p[13 + c * 3] * mapped[0]
                    + p[14 + c * 3] * mapped[1]
                    + p[15 + c * 3] * mapped[2];
            }
        }
    }
}
