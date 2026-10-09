use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
fn gain_contract(
    _: &GlobalContext,
    _: &(),
    image: Option<&BayerDesc>,
    gains: Option<&BayerGainDesc>,
) -> Result<(Option<BayerDesc>,)> {
    let (Some(image), Some(gains)) = (image, gains) else {
        return Ok((None,));
    };
    if image.phase != gains.phase || image.interpretation != gains.source {
        return Err(Error::Contract("Bayer gain coordinates/phase do not match image".into()));
    }
    let mut d = image.clone();
    d.interpretation = gains.target.clone();
    Ok((Some(d),))
}
/// Apply four site gains to Bayer samples in the matching response convention.
/// Gains are ordered by local 2x2 position. No guessed white balance or CFA.
#[crate::node(id="bayer-site-gains", name="White balance", category="color",contract=gain_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Bayer>>,
    gains: Read<'_, Device<BayerGains>>,
    output: Write<'_, Device<Bayer>>,
) -> Result<()> {
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    kernel::bayer_gains::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        gains.data.argument(),
        output.data.argument(),
        image.desc.extent.width as usize,
    );
    Ok(())
}

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use cubecl::prelude::*;
    #[cube(launch)]
    pub fn bayer_gains(input: &[f32], gains: &[f32], output: &mut [f32], width: usize) {
        let i = ABSOLUTE_POS;
        if i < input.len() {
            output[i] = input[i] * gains[((i / width) % 2) * 2 + (i % width) % 2];
        }
    }
}
