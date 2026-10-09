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
/// Box-average ColorRgb samples without changing their interpretation.
#[crate::node(id="reduce-rgb", name="RGB box reduction", category="geometry", contract=color_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<ColorRgb>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    dispatch_rgb(ctx, image, output, p.factor as u32)
}
