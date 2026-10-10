use super::{Settings, contract, dispatch_rgb};
use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn color_contract(
    global: &GlobalContext,
    p: &Settings,
    image: Option<&ImageDesc<Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    contract(global, p, image)
}
/// Reduce ColorRgb by box averaging without changing its interpretation.
/// The factor applies to both dimensions; partial edge blocks use available
/// samples. Output retains the input coordinates, encoding and scale.
#[crate::node(id="geometry.reduce-color-rgb", name="RGB box reduction", category="Geometry", contract=color_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<ColorRgb>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    dispatch_rgb(ctx, image, output, p.factor as u32)
}
