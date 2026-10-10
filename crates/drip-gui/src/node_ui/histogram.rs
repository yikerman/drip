use super::scopes::histogram::histogram;
use super::scopes::{ExposureSettings, Histogram, scope_contract};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};
#[cfg(test)]
pub use observe::add;
use std::sync::Arc;
/// Plot ColorRgb channel counts by exposure. Requires identity encoding and
/// applies no color conversion. Exposure is measured relative to sample value 1;
/// the density scale controls counts, not the exposure axis.
#[drip::node(id="view.histogram", name="Histogram", category="View", contract=scope_contract)]
pub fn observe(
    _: &KernelContext<'_>,
    _: &ExposureSettings,
    image: Read<'_, Device<ColorRgb>>,
) -> Result<()> {
    let _ = image;
    Ok(())
}
struct HistogramGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for HistogramGui {
    type Parameters = ExposureSettings;
    type Presentation = Arc<Histogram>;
    const ID: &'static str = "view.histogram";
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        histogram(p, (&ctx.device_image::<drip::node::data::Color>(inputs)?,), ctx)
    });
}
