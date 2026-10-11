use super::scopes::waveform::waveform;
use super::scopes::{ExposureRange, Scale, Scope};
use drip::{Result, node::data::*, ports::*, runtime::KernelContext};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

/// Histogram's exposure range; bin brightness defaults to logarithmic.
#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
pub struct Settings {
    #[param(flatten)]
    #[serde(flatten)]
    pub range: ExposureRange,
    /// Scale bin brightness linearly or logarithmically; the exposure axis remains in stops.
    #[param(default = Scale::Log)]
    #[label("Density scale")]
    pub scale: Scale,
}
/// Plot linear RGB channel exposure against horizontal image position, without a
/// color conversion; camera RGB shows native channels. Exposure is measured
/// relative to sample value 1; the density scale controls bin brightness.
#[drip::node(id = "view.waveform", name = "Waveform", category = "View")]
pub fn observe(_: &KernelContext<'_>, _: &Settings, _image: Read<'_, Device<Rgb>>) -> Result<()> {
    Ok(())
}
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for observe::Definition {
    type Presentation = Arc<Scope>;
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        waveform(&p.range, p.scale, (&super::data::DeviceRgb::new(&inputs.image),), ctx)
    });
}

#[cfg(test)]
mod tests {
    use crate::model::{Project, Registry};
    use serde_json::json;

    #[test]
    fn waveform_defaults_and_reset_are_log_but_saved_linear_is_retained() {
        let definition = Registry.get("view.waveform").unwrap();
        assert_eq!((definition.defaults)()["scale"], "log");
        assert_eq!(definition.param("scale").unwrap().kind.default_value(), "log");
        let mut project = Project::default();
        let node = project.graph.add_node(definition).unwrap();
        project.graph.set_param(node, "scale", json!("linear")).unwrap();
        project.graph.set_param(node, "min_ev", json!(-10)).unwrap();
        let reopened = Project::from_json(&project.to_json().unwrap(), &Registry).unwrap();
        let node = reopened.graph.find(definition.name).unwrap();
        assert_eq!(
            reopened.graph.node(node).unwrap().params,
            json!({"min_ev":-10,"max_ev":4,"scale":"linear"})
        );
        let histogram = Registry.get("view.histogram").unwrap();
        assert_eq!((histogram.defaults)()["scale"], "linear");
        assert_eq!(histogram.param("scale").unwrap().kind.default_value(), "linear");
    }
}
