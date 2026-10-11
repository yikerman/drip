use super::scopes::waveform::waveform;
use super::scopes::{ExposureSettings, Scale, Scope};
use drip::{Result, node::data::*, param::ParamKind, ports::*, runtime::KernelContext};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Lower exposure bound in stops relative to sample value 1. Values below it enter the first bin.
    #[param(ParamKind::Int { min: -24, max: -1, default: -12 })]
    #[label("Minimum exposure (EV)")]
    pub min_ev: i64,
    /// Upper exposure bound in stops relative to sample value 1. Values above it enter the last bin.
    #[param(ParamKind::Int { min: 1, max: 10, default: 4 })]
    #[label("Maximum exposure (EV)")]
    pub max_ev: i64,
    /// Scale bin brightness linearly or logarithmically; the exposure axis remains in stops.
    #[param(Scale::Log.schema())]
    #[label("Density scale")]
    pub scale: Scale,
}
impl Default for Settings {
    fn default() -> Self {
        Self { min_ev: -12, max_ev: 4, scale: Scale::Log }
    }
}
impl Settings {
    pub(super) fn exposure_settings(&self) -> ExposureSettings {
        ExposureSettings { min_ev: self.min_ev, max_ev: self.max_ev, scale: self.scale }
    }
}
fn validate(p: &Settings) -> Result<()> {
    super::scopes::exposure_range(&p.exposure_settings())
}
/// Plot linear RGB channel exposure against horizontal image position, without a
/// color conversion; camera RGB shows native channels. Exposure is measured
/// relative to sample value 1; the density scale controls bin brightness.
#[drip::node(id = "view.waveform", name = "Waveform", category = "View")]
pub fn observe(_: &KernelContext<'_>, _: &Settings, image: Read<'_, Device<Rgb>>) -> Result<()> {
    let _ = image;
    Ok(())
}
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for observe::Definition {
    type Presentation = Arc<Scope>;
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        waveform(p.exposure_settings(), (&super::data::DeviceRgb::new(&inputs.image),), ctx)
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
