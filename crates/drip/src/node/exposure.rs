use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
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
#[crate::node(id = "exposure", name="Exposure", tags = [tag::LOOK], contract = exposure_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    params: &Exposure,
    image: Read<'_, Device<Rgb>>,
    output: Write<'_, Device<Rgb>>,
) -> Result<()> {
    let (count, dim) = dispatch_dims(image.desc.extent.pixels() * 3);
    kernel::exposure::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        output.data.argument(),
        params.ev.exp2(),
    );
    Ok(())
}

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use cubecl::prelude::*;
    #[cube(launch)]
    pub fn exposure(input: &[f32], output: &mut [f32], gain: f32) {
        let i = ABSOLUTE_POS;
        if i < input.len() {
            output[i] = input[i] * gain;
        }
    }
}
