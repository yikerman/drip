use super::{Settings, bayer_extent, dispatch_bayer};
use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn bayer_contract(
    _: &GlobalContext,
    p: &Settings,
    input: Option<&BayerDesc>,
) -> Result<(Option<BayerDesc>,)> {
    if !(1..=256).contains(&p.factor) {
        return Err(Error::Contract("box factor must be 1..=256".into()));
    }
    let output = input
        .map(|d| {
            if p.factor > 1 && (d.extent.width < 2 || d.extent.height < 2) {
                return Err(Error::Contract("Bayer reduction needs all four CFA sites".into()));
            }
            Ok(BayerDesc { extent: bayer_extent(&d.extent, p.factor as u32), ..d.clone() })
        })
        .transpose()?;
    Ok((output,))
}

/// Reduce Bayer by averaging each of its four phase planes separately. Sensor
/// corrections belong before this spatial approximation; demosaic follows it.
/// Complete blocks are retained, with a minimum of one Bayer cell for small
/// inputs. Factor 1 preserves every sample, including incomplete edge cells.
/// This explicit operation uses its factor parameter, independently of the
/// global context's requested demosaic scale.
#[crate::node(id="reduce-bayer", name="Bayer box reduction", category="geometry", contract=bayer_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<Bayer>>,
    output: Write<'_, Device<Bayer>>,
) -> Result<()> {
    dispatch_bayer(ctx, &image, output.desc, output.data, p.factor as u32)
}
