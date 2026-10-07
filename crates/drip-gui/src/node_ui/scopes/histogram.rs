//! Histogram evaluation and its integer reduction.

use drip::image::Linearity;
use drip::node::{EvalContext, KernelError};
use drip::nodes::scopes::ExposureSettings;
use drip::ports::MatRef;
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
}

const BINS: usize = 256;

pub fn histogram(
    p: ExposureSettings,
    (image,): (MatRef<'_, 3, dyn Linearity>,),
    _: &EvalContext<'_>,
) -> Result<Arc<Histogram>, KernelError> {
    let (min, max) = (p.min_ev as f32, p.max_ev as f32);
    let thresholds = std::array::from_fn(|i| 2f32.powf(min + i as f32 * (max - min) / BINS as f32));
    let counts = count(&image.rgb().pixels, &thresholds);
    let log = super::logarithmic(p.scale);
    let histogram = Histogram { min_stop: min, max_stop: max, counts, log };
    Ok(Arc::new(histogram))
}

fn count(input: &[[f32; 3]], thresholds: &[f32; BINS]) -> Vec<[u32; 3]> {
    input
        .par_chunks(4096)
        .map(|pixels| {
            // Accumulate in place: passing this 3 KiB array through a fold per
            // pixel can copy it repeatedly. One partial per chunk amortizes it.
            let mut counts = [[0u32; 3]; BINS];
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
            || [[0; 3]; BINS],
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
        assert_eq!(count(&[], &[0.0; BINS]), vec![[0; 3]; BINS]);
    }
}

struct HistogramGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for HistogramGui {
    type Node = drip::nodes::scopes::HistogramNode;
    type Prepared = Arc<Histogram>;
    const NODE: &'static drip::node::TypedNode<Self::Node> = &drip::nodes::HISTOGRAM;
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(histogram);
}
