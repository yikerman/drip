use super::scopes::waveform::waveform;
use super::scopes::{ExposureSettings, Scope, camera_scope_contract};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};

use std::sync::Arc;
/// Plot CameraRgb native channel exposure against horizontal image position.
#[drip::node(id="view.camera-waveform", name="Camera waveform", category="view", contract=camera_scope_contract)]
pub fn observe(
    _: &KernelContext<'_>,
    _: &ExposureSettings,
    image: Read<'_, Cpu<CameraRgb>>,
) -> Result<()> {
    let _ = image;
    Ok(())
}
struct CameraWaveformGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for CameraWaveformGui {
    type Parameters = ExposureSettings;
    type Presentation = Arc<Scope>;
    const ID: &'static str = "view.camera-waveform";
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        waveform(p, (&*ctx.image::<drip::node::data::Camera>(inputs)?,), ctx)
    });
}
