//! Waveform: per-channel exposure against horizontal image position.
use super::{ExposureRange, SIZE, Scale, Scope, ScopeAxes, scope};
#[cfg(test)]
use crate::node_ui::data::HostImage;
use crate::node_ui::{GuiNode, Prepare};
use drip::registry::tag;
use drip::{Result, body::BinsBody, node::data::*, ports::*, runtime::GlobalContext};
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
fn contract(_: &GlobalContext, _: &Settings, image: &RgbDesc) -> Result<(CountsDesc,)> {
    Ok((super::counts(SIZE * SIZE, 3, image),))
}
/// Plot linear RGB channel exposure against horizontal image position, without a
/// color conversion, so camera RGB shows native channels. Exposure is measured
/// relative to sample value 1. The density scale controls bin brightness.
#[drip::node(id = "waveform", name = "Waveform", tags = [tag::VIEW, tag::SCOPE], contract = contract, bins = WAVEFORM)]
pub fn observe(
    p: &Settings,
    _image: Point<'_, Rgb>,
    _counts: Point<'_, Counts>,
) -> Result<[f32; SIZE]> {
    Ok(super::edges(&p.range))
}

/// Row-major `SIZE × SIZE` bins, exposure rows from the top and position
/// columns, with the exposure bin edges as constants.
static WAVEFORM: BinsBody = BinsBody {
    name: "drip-gui::waveform",
    constants: SIZE,
    bins: SIZE * SIZE,
    channels: 3,
    bin: kernel::waveform::expand,
};

mod kernel {
    use crate::node_ui::scopes::stop;
    use cubecl::prelude::*;
    use drip::body::{Bin, Pixel, Position};

    /// Image column `x` falls in scope column `⌊256 · x / width⌋`, so the
    /// scope columns split the image columns as evenly as integers allow.
    #[cube]
    pub fn waveform(px: Pixel, at: Position, k: &[f32]) -> Bin {
        let column = at.x * 256u32 / at.width;
        Bin {
            c0: (255u32 - stop(px.r, k)) * 256u32 + column,
            c1: (255u32 - stop(px.g, k)) * 256u32 + column,
            c2: (255u32 - stop(px.b, k)) * 256u32 + column,
        }
    }
}

impl GuiNode for observe::Definition {
    type Presentation = Arc<Scope>;
    const PREPARE: Option<Prepare<Self>> =
        Some(Prepare::Outputs(|node, outputs, _| waveform(&node.parameters, &outputs.counts)));
}

pub fn waveform(settings: &Settings, counts: &Read<'_, Counts>) -> Result<Arc<Scope>> {
    let (min_stop, max_stop) = (settings.range.min_ev as f32, settings.range.max_ev as f32);
    Ok(scope(
        super::per_bin(counts)?,
        ScopeAxes::Waveform { min_stop, max_stop },
        super::logarithmic(settings.scale),
        counts.desc.coordinates.to_string(),
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
    use crate::node_ui::data::Placement;
    #[test]
    fn waveform_preserves_columns_channels_and_exposure() {
        let image = HostImage {
            width: 2,
            height: 2,
            plane: Placement::whole([2.0, 2.0]),
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
    use crate::model::Project;
    use serde_json::json;

    #[test]
    fn waveform_defaults_and_reset_are_log_but_saved_linear_is_retained() {
        let definition = &crate::node_ui::scopes::waveform::observe::KIND;
        assert_eq!((definition.defaults)()["scale"], "log");
        assert_eq!(definition.param("scale").unwrap().kind.default_value(), "log");
        let mut project = Project::default();
        let node = project.graph.add_node(definition).unwrap();
        project.graph.set_param(node, "scale", json!("linear")).unwrap();
        project.graph.set_param(node, "min_ev", json!(-10)).unwrap();
        let reopened = Project::from_json(&project.to_json().unwrap()).unwrap();
        let node = reopened.graph.find(definition.name).unwrap();
        assert_eq!(
            reopened.graph.node(node).unwrap().params,
            json!({"min_ev":-10,"max_ev":4,"scale":"linear"})
        );
        let histogram = &crate::node_ui::scopes::histogram::observe::KIND;
        assert_eq!((histogram.defaults)()["scale"], "linear");
        assert_eq!(histogram.param("scale").unwrap().kind.default_value(), "linear");
    }
}
