use super::reduce;
use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
mod kernel;
fn rcd_contract(
    global: &GlobalContext,
    _: &(),
    image: Option<&BayerDesc>,
) -> Result<(Option<ImageDesc<Camera>>,)> {
    Ok((image.map(|d| ImageDesc {
        extent: reduce::demosaic_extent(global, &d.extent),
        interpretation: d.interpretation.clone(),
    }),))
}
/// Ratio-corrected Bayer demosaic into CameraRgb. Clamps negative sensor samples
/// to zero as in the source algorithm.
/// Global scale averages CFA phase planes before RCD; scale 1 uses full detail.
/// Uses a ten-pixel bilinear border; Bayer phase is explicit. RCD direction ties
/// may differ slightly across backends. See the pinned darktable derivation.
#[crate::node(id="bayer-rcd", name="Demosaic", category="demosaic",contract=rcd_contract,references=[("RCD source", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/demosaicing/rcd.c")])]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Bayer>>,
    output: Write<'_, Device<CameraRgb>>,
) -> Result<()> {
    let reduced = reduce::for_demosaic(ctx, &image)?;
    let image = match &reduced {
        Some((desc, data)) => Read { desc, data },
        None => image,
    };

    let n = image.desc.extent.pixels();
    let w = image.desc.extent.width as usize;
    let h = image.desc.extent.height as usize;
    let phase = image.desc.phase as usize;
    let (count, dim) = dispatch_dims(n);
    let (vh, low, p, q, pq) = (
        ctx.scratch_f32(n)?,
        ctx.scratch_f32(n)?,
        ctx.scratch_f32(n)?,
        ctx.scratch_f32(n)?,
        ctx.scratch_f32(n)?,
    );
    let (rgb, other) = (ctx.scratch_f32(n * 3)?, ctx.scratch_f32(n * 3)?);
    let client = ctx.client()?;
    kernel::rcd_maps::launch(
        client,
        count.clone(),
        dim,
        image.data.argument(),
        vh.argument(),
        low.argument(),
        p.argument(),
        q.argument(),
        rgb.argument(),
        w,
        h,
        phase,
    );
    kernel::rcd_green::launch(
        client,
        count.clone(),
        dim,
        image.data.argument(),
        vh.argument(),
        low.argument(),
        rgb.argument(),
        w,
        h,
        phase,
    );
    kernel::rcd_pq::launch(
        client,
        count.clone(),
        dim,
        p.argument(),
        q.argument(),
        pq.argument(),
        w,
        h,
        phase,
    );
    kernel::rcd_opposite::launch(
        client,
        count.clone(),
        dim,
        rgb.argument(),
        pq.argument(),
        other.argument(),
        w,
        h,
        phase,
    );
    kernel::rcd_finish::launch(
        client,
        count,
        dim,
        image.data.argument(),
        other.argument(),
        vh.argument(),
        output.data.argument(),
        w,
        h,
        phase,
    );
    Ok(())
}
