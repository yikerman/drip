//! Histogram: per-channel counts over the exposure axis.

use super::{ExposureRange, SIZE, Scale};
use crate::node_ui::{GuiNode, Prepare};
use drip::registry::tag;
use drip::{Result, body::BinsBody, node::data::*, ports::*, runtime::GlobalContext};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use rayon::prelude::*;
use std::sync::Arc;

/// Pixel counts per channel over equal steps of log2 value (stops), which
/// suits linear data. Values at or below `2^min_stop`, zero and negative
/// included, fall in the first bin; values at or above `2^max_stop` in the last.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub min_stop: f32,
    pub max_stop: f32,
    pub counts: Vec<[u32; 3]>,
    /// Whether the view plots the counts on a log scale rather than linearly.
    pub log: bool,
    pub color_space: String,
}

#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
/// The exposure axis and how bin counts are scaled.
pub struct Settings {
    #[param(flatten)]
    #[serde(flatten)]
    pub range: ExposureRange,
    /// Scale sample counts linearly or logarithmically; the exposure axis remains in stops.
    #[param]
    #[label("Density scale")]
    pub scale: Scale,
}
fn contract(_: &GlobalContext, _: &Settings, image: &RgbDesc) -> Result<(CountsDesc,)> {
    Ok((super::counts(SIZE, 3, image),))
}
/// Plot linear RGB channel counts by exposure, without a color conversion, so
/// camera RGB shows native channels. Exposure is measured relative to sample
/// value 1. The density scale controls counts, not the exposure axis.
#[drip::node(id = "histogram", name = "Histogram", tags = [tag::VIEW, tag::SCOPE], contract = contract, bins = HISTOGRAM)]
pub fn observe(
    p: &Settings,
    _image: Point<'_, Rgb>,
    _counts: Point<'_, Counts>,
) -> Result<[f32; SIZE]> {
    Ok(super::edges(&p.range))
}

/// Each channel's exposure bin, with the bin edges as constants.
static HISTOGRAM: BinsBody = BinsBody {
    name: "drip-gui::histogram",
    constants: SIZE,
    bins: SIZE,
    channels: 3,
    bin: kernel::histogram::expand,
};

mod kernel {
    use crate::node_ui::scopes::stop;
    use cubecl::prelude::*;
    use drip::body::{Bin, Pixel, Position};

    #[cube]
    pub fn histogram(px: Pixel, _at: Position, k: &[f32]) -> Bin {
        Bin { c0: stop(px.r, k), c1: stop(px.g, k), c2: stop(px.b, k) }
    }
}

impl GuiNode for observe::Definition {
    type Presentation = Arc<Histogram>;
    const PREPARE: Option<Prepare<Self>> =
        Some(Prepare::Outputs(|node, outputs, _| histogram(&node.parameters, &outputs.counts)));
}

pub fn histogram(settings: &Settings, counts: &Read<'_, Counts>) -> Result<Arc<Histogram>> {
    Ok(Arc::new(Histogram {
        min_stop: settings.range.min_ev as f32,
        max_stop: settings.range.max_ev as f32,
        counts: super::per_bin(counts)?,
        log: super::logarithmic(settings.scale),
        color_space: counts.desc.coordinates.to_string(),
    }))
}

#[cfg(test)]
pub(super) fn count(input: &[[f32; 3]], thresholds: &[f32; SIZE]) -> Vec<[u32; 3]> {
    input
        .par_chunks(4096)
        .map(|pixels| {
            // Accumulate in place: passing this 3 KiB array through a fold per
            // pixel can copy it repeatedly. One partial per chunk amortizes it.
            let mut counts = [[0u32; 3]; SIZE];
            for pixel in pixels {
                for (c, &value) in pixel.iter().enumerate() {
                    // Comparing edges keeps exact boundaries independent of log rounding.
                    let bin = thresholds.partition_point(|&edge| value >= edge).saturating_sub(1);
                    counts[bin][c] += 1;
                }
            }
            counts
        })
        .reduce(
            || [[0; 3]; SIZE],
            |mut counts, partial| {
                // Private histograms avoid atomic contention; integer sums are exact.
                for (count, value) in counts.iter_mut().zip(partial) {
                    for c in 0..3 {
                        count[c] += value[c];
                    }
                }
                counts
            },
        )
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_agrees_at_every_bin_edge_across_workers() {
        let thresholds = std::array::from_fn(|i| 2f32.powf(-12.0 + i as f32 / 16.0));
        let mut input = Vec::new();
        let mut expected = vec![[0; 3]; 256];
        for _ in 0..33 {
            for (i, &edge) in thresholds.iter().enumerate() {
                input.push([edge.next_down(), edge, edge.next_up()]);
                expected[i.saturating_sub(1)][0] += 1;
                expected[i][1] += 1;
                expected[i][2] += 1;
            }
        }
        input.extend([[-1.0, 0.0, f32::MIN_POSITIVE], [16.0, 1e20, f32::MAX]]);
        for bin in [0, 255] {
            for count in &mut expected[bin] {
                *count += 1;
            }
        }
        let counts = count(&input, &thresholds);
        assert_eq!(counts, expected);
    }
    #[test]
    fn empty_image() {
        assert_eq!(count(&[], &[0.0; SIZE]), vec![[0; 3]; SIZE]);
    }
}
