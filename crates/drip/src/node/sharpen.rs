//! Sharpen: an unsharp mask as a stencil. The host function turns the
//! Gaussian's standard deviation into the stencil's reach, and the body blurs
//! the window around each sample and adds back the detail. Pyramid blur uses
//! it at amount −1 as its blur.
use crate::{
    Error, Result,
    body::{Border, Stencil, StencilBody},
    node::data::*,
    ports::*,
    registry::tag,
    runtime::GlobalContext,
};
pub use compute::{Definition, Ports, add, definition};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Standard deviation of the Gaussian blur, in full-detail pixels.
    #[param(default = 2.0, range = 0.25..=4.0)]
    pub radius: f32,
    /// How much of the detail to add. Negative values soften, and −1 is the
    /// blur itself.
    #[param(default = 0.5, range = -1.0..=4.0)]
    pub amount: f32,
}
fn validate(p: &Settings) -> Result<()> {
    if !(0.25..=4.0).contains(&p.radius) || !(-1.0..=4.0).contains(&p.amount) {
        return Err(Error::Contract(
            "sharpen needs a radius in [0.25, 4] and amount in [-1, 4]".into(),
        ));
    }
    Ok(())
}

fn contract(_: &GlobalContext, _: &Settings, image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Sharpen RGB with an unsharp mask: each sample plus the amount times its
/// difference from a Gaussian blur of its neighbourhood, after darktable's
/// sharpen. The blur's standard deviation is the radius in full-detail
/// pixels, and it reaches 2.5 radii. Negative amounts soften, and −1 outputs
/// the blur. Coordinates are kept.
///
/// Each channel is sharpened on its own in linear light, so strong amounts
/// leave coloured halos along saturated edges and overshoot below zero beside
/// highlights, which are kept. Past the image's edge the blur repeats the
/// edge samples. A preview computes the same Gaussian on its own samples, so
/// a radius below one preview sample shows less sharpening than full detail.
#[crate::node(id = "sharpen", name = "Sharpen", tags = [tag::LOOK], contract = contract, stencil = UNSHARP, references = [
    ("darktable developers, sharpen.c, 2026", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/sharpen.c"),
])]
pub fn compute(
    p: &Settings,
    _image: Point<'_, Rgb>,
    _output: Point<'_, Rgb>,
) -> Result<Stencil<2>> {
    Ok(Stencil { radius: 2.5 * p.radius, constants: [p.radius, p.amount] })
}

static UNSHARP: StencilBody = StencilBody {
    name: "sharpen::unsharp",
    constants: 2,
    border: Border::Repeat,
    expand: kernel::unsharp::expand,
};

mod kernel {
    use crate::body::{Pixel, Window};
    use cubecl::prelude::*;

    /// `k[0]` is the standard deviation in full-detail pixels and `k[1]` the
    /// amount. The weights are normalized over the window, which truncates
    /// the Gaussian at the reach as darktable does.
    #[cube]
    pub fn unsharp(w: &Window, k: &[f32]) -> Pixel {
        let sx = k[0] * w.rate_x;
        let sy = k[0] * w.rate_y;
        let ax = -0.5f32 / (sx * sx);
        let ay = -0.5f32 / (sy * sy);
        let mut total = 0.0f32;
        let mut r = 0.0f32;
        let mut g = 0.0f32;
        let mut b = 0.0f32;
        for dy in -w.radius_y..w.radius_y + 1 {
            let fy = f32::cast_from(dy);
            let wy = f32::exp(ay * fy * fy);
            for dx in -w.radius_x..w.radius_x + 1 {
                let fx = f32::cast_from(dx);
                let weight = wy * f32::exp(ax * fx * fx);
                let px = w.at(dx, dy);
                r += weight * px.r;
                g += weight * px.g;
                b += weight * px.b;
                total += weight;
            }
        }
        let px = w.at(0i32, 0i32);
        Pixel {
            r: px.r + k[1] * (px.r - r / total),
            g: px.g + k[1] * (px.g - g / total),
            b: px.b + k[1] * (px.b - b / total),
        }
    }
}
