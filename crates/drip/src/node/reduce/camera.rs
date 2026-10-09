use super::{Settings, contract};
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
    _: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Cpu<CameraRgb>>,
    output: Write<'_, Cpu<CameraRgb>>,
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
