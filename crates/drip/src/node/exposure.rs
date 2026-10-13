use crate::{Error, Result, node::data::*, ports::*, registry::tag, runtime::GlobalContext};
pub use compute::{Definition, Ports, add, definition};
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Exposure {
    /// Exposure adjustment in stops, applied as a positive multiplier.
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
/// Adjust RGB exposure by multiplying linear samples by 2^EV. Output retains
/// the input coordinates. Negative and above-one samples are allowed; this
/// operation does not impose a scene/display processing order.
#[crate::node(id = "exposure", name = "Exposure", tags = [tag::LOOK], contract = exposure_contract, map = kernel::exposure)]
pub fn compute(
    params: &Exposure,
    _image: Point<'_, Rgb>,
    _output: Point<'_, Rgb>,
) -> Result<[f32; 1]> {
    Ok([params.ev.exp2()])
}

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use crate::shape::Pixel;
    use cubecl::prelude::*;
    #[cube]
    pub fn exposure(px: Pixel, k: &[f32]) -> Pixel {
        Pixel { r: px.r * k[0], g: px.g * k[0], b: px.b * k[0] }
    }
}
