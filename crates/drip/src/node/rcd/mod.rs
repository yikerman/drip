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
/// Demosaic Bayer to CameraRgb with ratio-corrected demosaicing (RCD).
/// Requires an explicit Bayer phase. Requested preview detail first reduces each
/// CFA phase plane; full detail preserves the input grid. Negative samples are
/// clipped to zero, and a ten-pixel border uses bilinear interpolation.
/// Adapted from darktable RCD; direction ties may differ slightly across backends.
#[crate::node(id="demosaic.rcd", name="RCD demosaic", category="Demosaic",contract=rcd_contract,references=[("RCD source", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/demosaicing/rcd.c")])]
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
    let (vh, low, p, q) =
        (ctx.scratch_f32(n)?, ctx.scratch_f32(n)?, ctx.scratch_f32(n)?, ctx.scratch_f32(n)?);
    let rgb = ctx.scratch_f32(n * 3)?;
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
    // Release scratch after its final dispatch so the runtime can reuse storage
    // for later passes. Queued work owns its bindings; no host wait is needed.
    drop(low);
    let pq = ctx.scratch_f32(n)?;
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
    drop((p, q));
    let other = ctx.scratch_f32(n * 3)?;
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
    drop((rgb, pq));
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
