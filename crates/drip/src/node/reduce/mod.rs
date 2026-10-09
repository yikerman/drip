//! Explicit box reduction. Partial edge blocks use only their available pixels.
use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Width and height of each averaging block. Partial edge blocks are retained.
    #[param(crate::param::ParamKind::Int { min:1, max:256, default:2 })]
    pub factor: i64,
}
impl Default for Settings {
    fn default() -> Self {
        Self { factor: 2 }
    }
}
fn contract<I: Interpretation>(
    _: &GlobalContext,
    p: &Settings,
    input: Option<&ImageDesc<I>>,
) -> Result<(Option<ImageDesc<I>>,)> {
    if !(1..=256).contains(&p.factor) {
        return Err(Error::Contract("box factor must be 1..=256".into()));
    }
    Ok((input.map(|d| ImageDesc {
        extent: Extent {
            width: d.extent.width.div_ceil(p.factor as u32),
            height: d.extent.height.div_ceil(p.factor as u32),
        },
        interpretation: d.interpretation.clone(),
    }),))
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
    input: &Read<'_, Device<Bayer>>,
    desc: &BayerDesc,
    output: &F32Buffer<Bayer>,
    factor: u32,
) -> Result<()> {
    let (count, dim) = crate::runtime::dispatch_dims(desc.extent.pixels());
    crate::node::shared_kernel::reduce_bayer::launch(
        ctx.client()?,
        count,
        dim,
        input.data.argument(),
        output.argument(),
        input.desc.extent.width as usize,
        input.desc.extent.height as usize,
        desc.extent.width as usize,
        factor as usize,
    );
    Ok(())
}

/// Demosaic owns the request's reduction. This is local scratch, not a graph
/// operation or port transfer. The same kernel serves the explicit reducer.
pub(super) fn for_demosaic(
    ctx: &KernelContext<'_>,
    input: &Read<'_, Device<Bayer>>,
) -> Result<Option<(BayerDesc, F32Buffer<Bayer>)>> {
    let extent = demosaic_extent(ctx.global, &input.desc.extent);
    if extent == input.desc.extent {
        return Ok(None);
    }
    let desc = BayerDesc { extent, ..input.desc.clone() };
    let data = ctx.allocate::<Device<Bayer>>(&desc)?;
    dispatch_bayer(ctx, input, &desc, &data, ctx.global.scale)?;
    Ok(Some((desc, data)))
}

pub(super) fn demosaic_extent(global: &GlobalContext, input: &Extent) -> Extent {
    if input.width < 2 || input.height < 2 {
        return input.clone();
    }
    bayer_extent(input, global.scale)
}

pub mod bayer;
pub mod camera;
pub mod color;
