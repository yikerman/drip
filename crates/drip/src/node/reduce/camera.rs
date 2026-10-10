use super::{Settings, contract, dispatch_rgb};
use crate::{
    Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn camera_contract(
    global: &GlobalContext,
    p: &Settings,
    image: Option<&ImageDesc<Camera>>,
) -> Result<(Option<ImageDesc<Camera>>,)> {
    contract(global, p, image)
}
/// Reduce CameraRgb by box averaging without a color conversion.
/// The factor applies to both dimensions; partial edge blocks use available
/// samples. Output retains the input interpretation.
#[crate::node(id="geometry.reduce-camera-rgb", name="Camera RGB box reduction", category="Geometry", contract=camera_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<CameraRgb>>,
    output: Write<'_, Device<CameraRgb>>,
) -> Result<()> {
    dispatch_rgb(ctx, image, output, p.factor as u32)
}
