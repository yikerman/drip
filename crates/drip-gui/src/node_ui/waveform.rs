use super::scopes::waveform::waveform;
use super::scopes::{ExposureSettings, Scope, scope_contract};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};

use std::sync::Arc;
/// Plot identity-encoded ColorRgb channel exposure against horizontal image position.
#[drip::node(id="view.waveform", name="Waveform", category="view", contract=scope_contract)]
pub fn observe(
    _: &KernelContext<'_>,
    _: &ExposureSettings,
    image: Read<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    let _ = image;
    Ok(())
}
struct WaveformGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for WaveformGui {
    type Parameters = ExposureSettings;
    type Presentation = Arc<Scope>;
    const ID: &'static str = "view.waveform";
    const PREPARE: Option<crate::node_ui::Prepare<Self>> =
        Some(|p, inputs, ctx| waveform(p, (&*ctx.image::<drip::node::data::Color>(inputs)?,), ctx));
}
