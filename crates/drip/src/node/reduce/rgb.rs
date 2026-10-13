use super::Settings;
use crate::registry::tag;
use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn contract(_: &GlobalContext, p: &Settings, image: &RgbDesc) -> Result<(RgbDesc,)> {
    let factor = p.factor as u32;
    let reduce =
        |e: &Extent| Extent { width: e.width.div_ceil(factor), height: e.height.div_ceil(factor) };
    Ok((RgbDesc { extent: reduce(&image.extent), full: reduce(&image.full), ..image.clone() },))
}
/// Reduce RGB by box averaging without changing its coordinates. The factor
/// applies to both dimensions; partial edge blocks use available samples.
#[crate::node(id="reduce-rgb", name="RGB box reduction", tags = [tag::GEOMETRY], contract=contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Rgb>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let (count, dim) = crate::runtime::dispatch_dims(output.desc.extent.pixels() * 3);
    crate::node::shared_kernel::reduce_rgb::launch(
        ctx.client()?,
        count,
        dim,
        image.data.device(ctx)?.argument(),
        output.device(ctx)?.argument(),
        image.desc.extent.width as usize,
        image.desc.extent.height as usize,
        output.desc.extent.width as usize,
        p.factor as usize,
    );
    Ok(())
}
