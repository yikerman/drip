use super::scopes::histogram::histogram;
use super::scopes::{ExposureSettings, Histogram, camera_scope_contract};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};
#[cfg(test)]
pub use observe::add;
use std::sync::Arc;
/// Count CameraRgb native channels by exposure, without interpreting camera primaries as a display space.
#[drip::node(id="view.camera-histogram", name="Camera histogram", category="view", contract=camera_scope_contract)]
pub fn observe(
    _: &KernelContext<'_>,
    _: &ExposureSettings,
    image: Read<'_, Cpu<CameraRgb>>,
) -> Result<()> {
    let _ = image;
    Ok(())
}
struct CameraHistogramGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for CameraHistogramGui {
    type Parameters = ExposureSettings;
    type Presentation = Arc<Histogram>;
    const ID: &'static str = "view.camera-histogram";
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        histogram(p, (&*ctx.image::<drip::node::data::Camera>(inputs)?,), ctx)
    });
}
