use super::scopes::histogram::histogram;
use super::scopes::{ExposureSettings, Histogram, camera_scope_contract};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};
#[cfg(test)]
pub use observe::add;
use std::sync::Arc;
/// Plot CameraRgb native-channel counts by exposure, without a color conversion.
/// Exposure is measured relative to sample value 1; the density scale controls
/// counts, not the exposure axis.
#[drip::node(id="view.camera-histogram", name="Camera histogram", category="View", contract=camera_scope_contract)]
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
