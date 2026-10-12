use crate::{
    Error, Result,
    node::{camera, data::*},
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
use std::sync::Arc;
fn gain_contract(
    _: &GlobalContext,
    _: &(),
    image: &BayerDesc,
    metadata: &Arc<drip_raw::Metadata>,
) -> Result<(BayerDesc, BayerLevelsDesc)> {
    if image.response.camera != Camera::of(metadata) || image.phase != camera::phase(metadata)? {
        return Err(Error::Contract("Bayer camera/phase differ from the capture metadata".into()));
    }
    let response = Response {
        camera: image.response.camera.clone(),
        balance: Some(total_balance(image, metadata)?),
    };
    let levels = BayerLevelsDesc { phase: image.phase, response: response.clone() };
    Ok((BayerDesc { response, ..image.clone() }, levels))
}
/// As-shot gains composed with any balance the samples already carry.
fn total_balance(image: &BayerDesc, metadata: &drip_raw::Metadata) -> Result<[f32; 4]> {
    let gains = camera::as_shot_balance(metadata)?;
    let prior = image.response.balance.unwrap_or([1.; 4]);
    Ok(std::array::from_fn(|c| prior[c] * gains[c]))
}
/// Apply the capture's as-shot white balance to Bayer samples and derive their
/// clipping levels. Outputs Bayer and BayerLevels in the same balanced
/// coordinates; gains compose with a balance the input already carries. No
/// gains or CFA are inferred: missing as-shot balance prevents evaluation.
#[crate::node(id="bayer-site-gains", name="Bayer site gains", tags = [tag::INPUT_TRANSFORM, tag::BAYER_RAW],contract=gain_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Bayer>>,
    metadata: Read<'_, Cpu<CaptureMetadata>>,
    output: Write<'_, Device<Bayer>>,
    levels: Write<'_, Cpu<BayerLevels>>,
) -> Result<()> {
    let m = metadata.data;
    let clipping = camera::site_levels(m)?;
    let total = camera::sites(m, total_balance(image.desc, m)?);
    *levels.data = std::array::from_fn(|i| clipping[i] * total[i]);
    let gains = ctx.constants(&camera::sites(m, camera::as_shot_balance(m)?))?;
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    kernel::bayer_gains::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        gains.argument(),
        output.data.argument(),
        image.desc.extent.width as usize,
    );
    Ok(())
}

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use crate::node::shared_kernel::site;
    use cubecl::prelude::*;
    #[cube(launch)]
    pub fn bayer_gains(input: &[f32], gains: &[f32], output: &mut [f32], width: usize) {
        let i = ABSOLUTE_POS;
        if i < input.len() {
            output[i] = input[i] * gains[site(i / width, i % width)];
        }
    }
}
