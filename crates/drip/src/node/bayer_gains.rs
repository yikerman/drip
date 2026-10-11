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
    metadata: Option<&MetadataDesc>,
) -> Result<(Option<BayerDesc>, Option<BayerLevelDesc>)> {
    let (Some(image), Some(calibration)) = (image, metadata.and_then(|m| m.bayer.as_ref())) else {
        return Ok((None, None));
    };
    if image.phase != calibration.phase || image.interpretation != calibration.source {
        return Err(Error::Contract("Bayer gain coordinates/phase do not match image".into()));
    }
    let Some(target) = &calibration.balanced else {
        return Ok((None, None));
    };
    let mut d = image.clone();
    d.interpretation = target.clone();
    Ok((Some(d), Some(BayerLevelDesc { phase: image.phase, interpretation: target.clone() })))
}
/// Apply CaptureMetadata's site gains to Bayer samples and their clipping levels.
/// The image must match the calibration's response coordinates and local CFA phase.
/// Outputs are Bayer and BayerLevels in the same gain-adjusted interpretation.
/// No gains or CFA are inferred; missing calibration prevents evaluation.
#[crate::node(id="sensor.bayer-site-gains", name="Bayer site gains", category="Sensor",contract=gain_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Bayer>>,
    metadata: Read<'_, Cpu<CaptureMetadata>>,
    output: Write<'_, Device<Bayer>>,
    levels: Write<'_, Cpu<BayerLevels>>,
) -> Result<()> {
    let calibration = metadata
        .data
        .bayer
        .as_ref()
        .ok_or_else(|| Error::Contract("metadata has no Bayer calibration".into()))?;
    let gains = &calibration
        .gains
        .as_ref()
        .ok_or_else(|| Error::Contract("metadata has no Bayer site gains".into()))?
        .values;
    *levels.data = std::array::from_fn(|i| calibration.levels[i] * gains[i]);
    let coefficients = DeviceBuffer::<()>::new(
        ctx.client()?.create(cubecl::bytes::Bytes::from_elems(gains.to_vec())),
    );
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    kernel::bayer_gains::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        coefficients.argument(),
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
