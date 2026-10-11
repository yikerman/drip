//! Waveform reduction shared by color and camera scopes.
#[cfg(test)]
use super::SIZE;
use super::{ExposureRange, Scale, Scope, ScopeAxes, scope};
#[cfg(test)]
use crate::node_ui::data::HostImage;
use crate::node_ui::data::{DeviceRgb, PrepareContext};
use drip::Error as KernelError;
use drip::{node::data::*, ports::*, runtime::KernelContext};
#[cfg(test)]
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The exposure axis; bin brightness defaults to logarithmic.
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
pub fn observe(
    _: &KernelContext<'_>,
    _: &Settings,
    _image: Read<'_, Device<Rgb>>,
) -> drip::Result<()> {
    Ok(())
}
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for observe::Definition {
    type Presentation = Arc<Scope>;
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|node, inputs, ctx| {
        let p = &node.parameters;
        waveform(&p.range, p.scale, &DeviceRgb::new(&inputs.image), ctx)
    });
}

pub fn waveform(
    range: &ExposureRange,
    scale: Scale,
    image: &DeviceRgb,
    ctx: &PrepareContext,
) -> Result<Arc<Scope>, KernelError> {
    let (min, max) = (range.min_ev as f32, range.max_ev as f32);
    let counts = super::device::exposure(image, ctx, min, max, true)?;
    Ok(scope(
        counts,
        ScopeAxes::Waveform { min_stop: min, max_stop: max },
        super::logarithmic(scale),
        image.desc.coordinates.to_string(),
    ))
}

#[cfg(test)]
pub(super) fn waveform_counts(image: &HostImage, min: f32, max: f32) -> Vec<[u32; 3]> {
    let edges: [f32; SIZE] =
        std::array::from_fn(|i| 2f32.powf(min + i as f32 * (max - min) / SIZE as f32));
    // Each worker owns one scope column, avoiding full-grid partial histograms
    // and atomic increments. Every source column belongs to exactly one bucket.
    let columns: Vec<Vec<[u32; 3]>> = (0..SIZE)
        .into_par_iter()
        .map(|x| {
            let mut counts = vec![[0; 3]; SIZE];
            let start = (x * image.width).div_ceil(SIZE);
            let end = ((x + 1) * image.width).div_ceil(SIZE);
            for y in 0..image.height {
                for pixel in &image.pixels[y * image.width + start..y * image.width + end] {
                    for (c, &value) in pixel.iter().enumerate() {
                        let bin = edges.partition_point(|&edge| value >= edge).saturating_sub(1);
                        counts[SIZE - 1 - bin][c] += 1;
                    }
                }
            }
            counts
        })
        .collect();
    (0..SIZE * SIZE).map(|i| columns[i % SIZE][i / SIZE]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waveform_preserves_columns_channels_and_exposure() {
        let image = HostImage {
            width: 2,
            height: 2,
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: vec![[1.0, 0.0, 16.0], [0.5; 3], [1.0, -1.0, 32.0], [0.5; 3]].into(),
        };
        let counts = waveform_counts(&image, -12.0, 4.0);
        assert_eq!(counts.iter().flatten().sum::<u32>(), 12);
        assert_eq!(counts[63 * SIZE][0], 2);
        assert_eq!(counts[255 * SIZE][1], 2);
        assert_eq!(counts[0][2], 2);
        assert_eq!(counts[79 * SIZE + 128], [2; 3]);
    }
}

#[cfg(test)]
mod defaults {
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
