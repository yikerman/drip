use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
#[derive(Clone, Default, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Exposure {
    /// Exposure adjustment in stops, applied as a positive multiplier.
    #[param(crate::param::ParamKind::Float { min: -32.0, max: 32.0, default: 0.0 })]
    pub ev: f32,
}
fn exposure_contract(
    _: &GlobalContext,
    p: &Exposure,
    image: Option<&ImageDesc<Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    if !p.ev.is_finite() || !p.ev.exp2().is_finite() || p.ev.exp2() == 0.0 {
        return Err(Error::Contract("exposure gain must be finite and positive".into()));
    }
    if image.is_some_and(|d| d.interpretation.encoding != Encoding::Identity) {
        return Err(Error::Contract("exposure requires identity-encoded RGB".into()));
    }
    Ok((image.cloned(),))
}
/// Multiply identity-encoded ColorRgb by 2^stops. Preserve its coordinates and
/// declared scale; permit negative and above-one samples. No scene/display order rule.
#[crate::node(id = "rgb-exposure", name="Exposure", category="color", contract = exposure_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    params: &Exposure,
    image: Read<'_, Device<ColorRgb>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    let (count, dim) = dispatch_dims(image.desc.extent.pixels() * 4);
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
            output[i] = if i % 4 == 3 { 0.0f32 } else { input[i] * gain };
        }
    }
}
