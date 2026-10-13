//! Exposure scales linear RGB by `2^EV` as a map. Auto exposure meters an
//! image's mean luminance with a reduction ([`meter`]), which any frontend
//! calls on the node's input and applies as new parameters.
use crate::{
    Error, Result,
    body::{MapPayload, ReduceBody},
    image::Image,
    node::{color, data::*},
    param::Parameters,
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Exposure {
    /// Brightness change in stops. Each stop doubles or halves the light.
    #[param(default = 0.0, range = -32.0..=32.0)]
    #[label("Exposure (EV)")]
    pub ev: f32,
}
fn validate(p: &Exposure) -> Result<()> {
    if !p.ev.is_finite() || !p.ev.exp2().is_finite() || p.ev.exp2() == 0.0 {
        return Err(Error::Contract("exposure gain must be finite and positive".into()));
    }
    Ok(())
}
fn exposure_contract(_: &GlobalContext, _: &Exposure, image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
/// Brighten or darken linear RGB by a number of stops. Values above 1 and
/// below 0 pass through unclipped.
#[crate::node(id = "exposure", name = "Exposure", tags = [tag::LOOK], contract = exposure_contract, map = kernel::exposure)]
pub fn compute(
    params: &Exposure,
    _image: Point<'_, Rgb>,
    _output: Point<'_, Rgb>,
) -> Result<[f32; 1]> {
    Ok([params.ev.exp2()])
}

/// The exposure that brings the mean Rec.2020 luminance of the samples
/// `image` holds to 18 % grey, for auto exposure. Bright or dark subjects
/// may need a manual adjustment after it. Fails when the mean is not
/// positive, or when the exposure leaves the parameter's range.
pub fn meter(image: &Read<'_, Rgb, Rec2020>, ctx: &KernelContext<'_>) -> Result<Exposure> {
    // Like darktable's area exposure mapping [1], compare linear luminance
    // against a target. Here the area is the whole image and the target is
    // exactly Y=0.18, rather than its default L*=50 (Y≈0.1842).
    // [1] darktable contributors, “exposure,” development user manual.
    // https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/exposure/
    let mean = mean_luminance(image, ctx)?;
    if !mean.is_finite() || mean <= 0.0 {
        return Err(Error::Contract(
            "auto exposure needs a finite, positive mean luminance".into(),
        ));
    }
    let params = Exposure { ev: (0.18 / mean).log2() as f32 };
    params
        .validate()
        .map_err(|_| Error::Contract("auto exposure gain is not representable".into()))?;
    Ok(params)
}

/// The mean luminance Y of the samples `image` holds.
pub fn mean_luminance(image: &Read<'_, Rgb, Rec2020>, ctx: &KernelContext<'_>) -> Result<f64> {
    // Weights divided by the pixel count before summing keep HDR headroom: a
    // plain f32 sum of a bright image overflows where its mean does not.
    let pixels = Rgb::lattice(image.desc).window.cells() as f64;
    let weights = color::rgb_to_xyz(color::REC2020, color::D65)[1].map(|v| (v / pixels) as f32);
    let image = Image::from(image.data.clone());
    Ok(f64::from(image.fold::<Rgb>(&MEAN_LUMINANCE, &weights, ctx)?[0]))
}

/// The weighted sum of the channels of every pixel, with the weights as its
/// three constants.
static MEAN_LUMINANCE: ReduceBody = ReduceBody {
    name: "exposure::mean-luminance",
    constants: 3,
    identity: &[0.0],
    accumulate: luminance::accumulate::expand,
    combine: luminance::combine::expand,
};

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use crate::body::Pixel;
    use cubecl::prelude::*;
    #[cube]
    pub fn exposure(px: Pixel, k: &[f32]) -> Pixel {
        Pixel { r: px.r * k[0], g: px.g * k[0], b: px.b * k[0] }
    }
}

mod luminance {
    use crate::body::Pixel;
    use cubecl::prelude::*;

    #[cube]
    pub fn accumulate(px: Pixel, k: &[f32], acc: &mut [f32]) {
        acc[0] += px.r * k[0] + px.g * k[1] + px.b * k[2];
    }

    #[cube]
    pub fn combine(acc: &mut [f32], other: &[f32]) {
        acc[0] += other[0];
    }
}
