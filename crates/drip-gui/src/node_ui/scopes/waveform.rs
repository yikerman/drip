//! Waveform reduction shared by color and camera scopes.
use super::{ExposureSettings, SIZE, Scope, ScopeAxes, scope};
use crate::node_ui::data::{PrepareContext, Rgb};
use drip::Error as KernelError;
use rayon::prelude::*;
use std::sync::Arc;
pub fn waveform(
    p: ExposureSettings,
    (image,): (&Rgb,),
    _: &PrepareContext,
) -> Result<Arc<Scope>, KernelError> {
    let (min, max) = (p.min_ev as f32, p.max_ev as f32);
    let counts = waveform_counts(image, min, max);
    Ok(scope(
        counts,
        ScopeAxes::Waveform { min_stop: min, max_stop: max },
        super::logarithmic(p.scale),
    ))
}

fn waveform_counts(image: &Rgb, min: f32, max: f32) -> Vec<[u32; 3]> {
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
        let image = Rgb {
            width: 2,
            height: 2,
            requested_scale: 1,
            pixels: std::sync::Arc::new(
                vec![[1.0, 0.0, 16.0], [0.5; 3], [1.0, -1.0, 32.0], [0.5; 3]].into(),
            ),
        };
        let counts = waveform_counts(&image, -12.0, 4.0);
        assert_eq!(counts.iter().flatten().sum::<u32>(), 12);
        assert_eq!(counts[63 * SIZE][0], 2);
        assert_eq!(counts[255 * SIZE][1], 2);
        assert_eq!(counts[0][2], 2);
        assert_eq!(counts[79 * SIZE + 128], [2; 3]);
    }
}
