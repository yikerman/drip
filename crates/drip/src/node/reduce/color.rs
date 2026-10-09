use super::{Settings, contract};
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
    _: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Cpu<ColorRgb>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    crate::node::shared_kernel::reduce_rgb(
        p.factor as usize,
        &image.desc.extent,
        image.data,
        &output.desc.extent,
        output.data,
    );
    Ok(())
}
