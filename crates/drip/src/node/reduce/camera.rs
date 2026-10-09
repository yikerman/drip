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
/// Box-average CameraRgb native samples without a color conversion.
#[crate::node(id="reduce-camera-rgb", name="Camera RGB box reduction", category="geometry", contract=camera_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<CameraRgb>>,
    output: Write<'_, Device<CameraRgb>>,
) -> Result<()> {
    dispatch_rgb(ctx, image, output, p.factor as u32)
}
