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

/// Shrink Bayer data by averaging blocks of each color separately, keeping the
/// mosaic pattern. Incomplete blocks at the right and bottom edges are
/// dropped. Apply sensor corrections such as Hot pixel removal before it.
// At least one Bayer cell is kept for small inputs, and factor 1 keeps
// incomplete edge cells too. The factor is independent of the requested
// preview detail.
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
