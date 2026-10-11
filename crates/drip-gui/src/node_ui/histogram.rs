use super::data::DeviceRgb;
use super::scopes::histogram::histogram;
use super::scopes::{ExposureSettings, Histogram};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};
#[cfg(test)]
pub use observe::add;
use std::sync::Arc;
/// Plot linear RGB channel counts by exposure, without a color conversion; camera
/// RGB shows native channels. Exposure is measured relative to sample value 1;
/// the density scale controls counts, not the exposure axis.
#[drip::node(id = "view.histogram", name = "Histogram", category = "View")]
pub fn observe(
    _: &KernelContext<'_>,
    _: &ExposureSettings,
    image: Read<'_, Device<Rgb>>,
) -> Result<()> {
    let _ = image;
    Ok(())
}
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for observe::Definition {
    type Presentation = Arc<Histogram>;
    const PREPARE: Option<crate::node_ui::Prepare<Self>> =
        Some(|p, inputs, ctx| histogram(p, (&DeviceRgb::new(&inputs.image),), ctx));
}
