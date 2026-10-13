use super::reduce;
use crate::registry::tag;
use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
mod kernel;
fn rcd_contract(global: &GlobalContext, _: &(), image: &BayerDesc) -> Result<(RgbDesc,)> {
    Ok((RgbDesc {
        extent: reduce::demosaic_extent(global, &image.extent),
        full: image.extent.clone(),
        coordinates: Coordinates::Camera(image.response.clone()),
    },))
}
/// Demosaic Bayer to camera RGB with ratio-corrected demosaicing (RCD).
/// Requires an explicit Bayer phase. Requested preview detail first reduces each
/// CFA phase plane; full detail preserves the input grid. Negative samples are
/// clipped to zero, and a ten-pixel border uses bilinear interpolation.
/// Adapted from darktable RCD; direction ties may differ slightly across backends.
#[crate::node(id="rcd-demosaic", name="RCD demosaic", tags = [tag::INPUT_TRANSFORM, tag::BAYER_RAW, tag::DEMOSAIC],contract=rcd_contract,references=[("RCD source", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/demosaicing/rcd.c")])]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Bayer>,
    output: Write<'_, Rgb, CameraRgb>,
) -> Result<()> {
    let reduced = reduce::for_demosaic(ctx, image.desc, image.data.device(ctx)?)?;
    let image = match &reduced {
        Some(raster) => Read::new(raster),
        None => image,
    };

    let n = image.desc.extent.pixels();
    let w = image.desc.extent.width as usize;
    let h = image.desc.extent.height as usize;
    let phase = image.desc.phase as usize;
    let (count, dim) = dispatch_dims(n);
    let (vh, low, p, q) =
        (ctx.scratch_32(n)?, ctx.scratch_32(n)?, ctx.scratch_32(n)?, ctx.scratch_32(n)?);
    let rgb = ctx.scratch_32(n * 3)?;
    let client = ctx.client()?;
    kernel::rcd_maps::launch(
        client,
        count.clone(),
        dim,
        image.data.device(ctx)?.argument(),
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
        image.data.device(ctx)?.argument(),
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
    let pq = ctx.scratch_32(n)?;
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
    let other = ctx.scratch_32(n * 3)?;
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
        image.data.device(ctx)?.argument(),
        other.argument(),
        vh.argument(),
        output.device(ctx)?.argument(),
        w,
        h,
        phase,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_the_full_detail_extent() {
        let response = Response {
            camera: Camera { make: "test".into(), model: "camera".into() },
            balance: None,
        };
        let image = BayerDesc {
            extent: Extent { width: 6024, height: 4016 },
            phase: BayerPhase::Rggb,
            response,
        };
        let (full,) = rcd_contract(&GlobalContext::default(), &(), &image).unwrap();
        assert_eq!((full.extent.clone(), full.full), (image.extent.clone(), image.extent.clone()));
        let (preview,) = rcd_contract(&GlobalContext { scale: 8 }, &(), &image).unwrap();
        assert_eq!(preview.extent, Extent { width: 752, height: 502 });
        assert_eq!(preview.full, image.extent);
    }
}
