use super::Settings;
use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn contract(_: &GlobalContext, p: &Settings, image: &RgbDesc) -> Result<(RgbDesc,)> {
    let factor = p.factor as u32;
    let extent = Extent {
        width: image.extent.width.div_ceil(factor),
        height: image.extent.height.div_ceil(factor),
    };
    Ok((RgbDesc { extent, ..image.clone() },))
}
/// Reduce RGB by box averaging without changing its coordinates. The factor
/// applies to both dimensions; partial edge blocks use available samples.
#[crate::node(id="geometry.reduce-rgb", name="RGB box reduction", category="Geometry", contract=contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<Rgb>>,
    output: Write<'_, Device<Rgb>>,
) -> Result<()> {
    let (count, dim) = crate::runtime::dispatch_dims(output.desc.extent.pixels() * 3);
    crate::node::shared_kernel::reduce_rgb::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        output.data.argument(),
        image.desc.extent.width as usize,
        image.desc.extent.height as usize,
        output.desc.extent.width as usize,
        p.factor as usize,
    );
    Ok(())
}
