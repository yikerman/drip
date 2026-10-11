use super::scopes::Scope;
use super::scopes::waveform::waveform;
use super::waveform::Settings;
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};

use std::sync::Arc;
fn camera_scope_contract(
    ctx: &drip::runtime::GlobalContext,
    p: &Settings,
    image: Option<&ImageDesc<Camera>>,
) -> Result<()> {
    super::scopes::camera_scope_contract(ctx, &p.exposure_settings(), image)
}
/// Plot CameraRgb native-channel exposure against horizontal image position.
/// No color conversion is applied. Exposure is measured relative to sample value 1;
/// the density scale controls bin brightness.
#[drip::node(id="view.camera-waveform", name="Camera waveform", category="View", contract=camera_scope_contract)]
pub fn observe(
    _: &KernelContext<'_>,
    _: &Settings,
    image: Read<'_, Device<CameraRgb>>,
) -> Result<()> {
    let _ = image;
    Ok(())
}
struct CameraWaveformGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for CameraWaveformGui {
    type Parameters = Settings;
    type Presentation = Arc<Scope>;
    const ID: &'static str = "view.camera-waveform";
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        waveform(
            p.exposure_settings(),
            (&ctx.device_image::<drip::node::data::Camera>(inputs)?,),
            ctx,
        )
    });
}
