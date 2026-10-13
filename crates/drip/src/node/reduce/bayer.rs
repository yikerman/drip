use super::{Settings, bayer_extent, dispatch_bayer};
use crate::registry::tag;
use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn bayer_contract(_: &GlobalContext, p: &Settings, input: &BayerDesc) -> Result<(BayerDesc,)> {
    if p.factor > 1 && (input.extent.width < 2 || input.extent.height < 2) {
        return Err(Error::Contract("Bayer reduction needs all four CFA sites".into()));
    }
    Ok((BayerDesc { extent: bayer_extent(&input.extent, p.factor as u32), ..input.clone() },))
}

/// Reduce Bayer by averaging each CFA phase plane separately. Output retains the
/// input interpretation and phase. Complete blocks are retained, with at least
/// one Bayer cell for small inputs; factor 1 preserves incomplete edge cells too.
/// The explicit factor is independent of requested preview detail. Apply sensor
/// corrections before this spatial approximation when physical correspondence matters.
#[crate::node(id="reduce-bayer", name="Bayer box reduction", tags = [tag::GEOMETRY, tag::BAYER_RAW], contract=bayer_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Bayer>,
    output: Write<'_, Bayer>,
) -> Result<()> {
    let input = (image.desc, image.data.device(ctx)?);
    dispatch_bayer(ctx, input, (output.desc, output.device(ctx)?), p.factor as u32)
}
