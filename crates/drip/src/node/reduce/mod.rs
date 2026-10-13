//! Explicit box reduction. Partial edge blocks use only their available pixels.
use crate::{
    Error, Result,
    node::data::*,
    runtime::{GlobalContext, KernelContext},
};
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Reduction factor in each dimension. Bayer averages each CFA phase separately.
    #[param(default = 2, range = 1..=256)]
    pub factor: i64,
}
fn validate(p: &Settings) -> Result<()> {
    if !(1..=256).contains(&p.factor) {
        return Err(Error::Contract("box factor must be 1..=256".into()));
    }
    Ok(())
}
/// Extent of a mosaic after averaging its phase planes independently.
/// Small mosaics keep at least one complete cell; factor one is an identity.
pub(super) fn bayer_extent(input: &Extent, factor: u32) -> Extent {
    if factor == 1 {
        return input.clone();
    }
    Extent {
        width: (input.width / (2 * factor) * 2).max(2),
        height: (input.height / (2 * factor) * 2).max(2),
    }
}

pub(super) fn dispatch_bayer(
    ctx: &KernelContext<'_>,
    (input_desc, input): (&BayerDesc, &DeviceBuffer<Bayer>),
    (desc, output): (&BayerDesc, &DeviceBuffer<Bayer>),
    factor: u32,
) -> Result<()> {
    let (count, dim) = crate::runtime::dispatch_dims(desc.extent.pixels());
    crate::node::shared_kernel::reduce_bayer::launch(
        ctx.client()?,
        count,
        dim,
        input.argument(),
        output.argument(),
        input_desc.extent.width as usize,
        input_desc.extent.height as usize,
        desc.extent.width as usize,
        factor as usize,
    );
    Ok(())
}

/// Demosaic owns the request's reduction. This is local scratch, not a graph
/// operation or port transfer. The same kernel serves the explicit reducer.
pub(super) fn for_demosaic(
    ctx: &KernelContext<'_>,
    input_desc: &BayerDesc,
    input: &DeviceBuffer<Bayer>,
) -> Result<Option<Raster<Bayer>>> {
    let extent = demosaic_extent(ctx.global, &input_desc.extent);
    if extent == input_desc.extent {
        return Ok(None);
    }
    let desc = BayerDesc { extent, ..input_desc.clone() };
    let output = ctx.device_buffer::<Bayer>(&desc)?;
    dispatch_bayer(ctx, (input_desc, input), (&desc, &output), ctx.global.scale)?;
    Raster::from_device(desc, output, ctx).map(Some)
}

pub(super) fn demosaic_extent(global: &GlobalContext, input: &Extent) -> Extent {
    if input.width < 2 || input.height < 2 {
        return input.clone();
    }
    bayer_extent(input, global.scale)
}

pub mod bayer;
pub mod rgb;
