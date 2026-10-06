//! Histogram evaluation and its integer reduction.

use crate::image::LinearThreeChannelMatrix;
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::Params;
use crate::ports::Read;
use crate::view::{Histogram, View};
use rayon::prelude::*;
use std::sync::Arc;

const BINS: usize = 256;

/// Counts stops from `min_ev` to `max_ev`, for plotting on a linear or log
/// `scale`. The bounds lie on either side of 0 EV, so the range is never empty
/// and always shows where 1.0 falls.
pub static HISTOGRAM: NodeKind = NodeKind::new::<HistogramNode>(
    "view.histogram",
    "histogram",
    super::EXPOSURE_PARAMS,
    &["image"],
    &[],
);

struct HistogramNode;
impl NodeKernel for HistogramNode {
    type Inputs = (Read<dyn LinearThreeChannelMatrix>,);
    type Outputs = ();
    fn eval(
        p: Params<'_>,
        (image,): (&dyn LinearThreeChannelMatrix,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let (min, max) = (p.int("min_ev") as f32, p.int("max_ev") as f32);
        let thresholds =
            std::array::from_fn(|i| 2f32.powf(min + i as f32 * (max - min) / BINS as f32));
        let counts = count(&image.rgb().pixels, &thresholds);
        let log = p.choice("scale") == "log";
        let histogram = Histogram { min_stop: min, max_stop: max, counts, log };
        Ok(Evaluated { outputs: (), view: Some(View::Histogram(Arc::new(histogram))) })
    }
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
