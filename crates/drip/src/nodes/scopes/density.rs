//! Photographic scopes: spatial RGB exposure and exposure-independent chromaticity.
//!
//! \[1\] CIE, "CIE 1976 uniform-chromaticity-scale diagram," e-ILV,
//!     term 17-23-073. <https://cie.co.at/eilvterm/17-23-073>
//! The vectorscope uses u′v′ directly, not darktable's lightness-scaled u*v*.
//! Negative RGB components are clipped only for chromaticity visualization.

use std::sync::Arc;

use rayon::prelude::*;

#[cfg(test)]
use crate::color::REC2020;
use crate::color::{self, D65};
use crate::image::{ColorspaceRgbMatrix, LinearThreeChannelMatrix, Rgb};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::Params;
use crate::ports::Read;
use crate::view::{Scope, ScopeAxes, View};

const SIZE: usize = 256;
const RADIUS: f32 = 0.5;

pub static WAVEFORM: NodeKind = NodeKind::new::<Waveform>(
    "view.waveform",
    "view",
    "Waveform",
    super::EXPOSURE_PARAMS,
    &["image"],
    &[],
);

struct Waveform;
impl NodeKernel for Waveform {
    type Inputs = (Read<dyn LinearThreeChannelMatrix>,);
    type Outputs = ();
    fn eval(
        p: Params<'_>,
        (image,): (&dyn LinearThreeChannelMatrix,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let (min, max) = (p.int("min_ev") as f32, p.int("max_ev") as f32);
        let counts = waveform_counts(image.rgb(), min, max);
        scope(
            counts,
            ScopeAxes::Waveform { min_stop: min, max_stop: max },
            p.choice("scale") == "log",
        )
    }
}

pub static VECTORSCOPE: NodeKind =
    NodeKind::new::<Vectorscope>("view.vectorscope", "view", "Vectorscope", &[], &["image"], &[]);

struct Vectorscope;
impl NodeKernel for Vectorscope {
    type Inputs = (Read<dyn ColorspaceRgbMatrix>,);
    type Outputs = ();
    fn eval(
        _: Params<'_>,
        (image,): (&dyn ColorspaceRgbMatrix,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let space = image.color_space();
        let matrix = &space.to_xyz_d65;
        let counts = vector_counts(&image.rgb().pixels, matrix);
        let primaries = color::transpose(*matrix).map(|primary| position(uv(primary)));
        scope(counts, ScopeAxes::Vectorscope { primaries, color_space: space.name }, true)
    }
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

fn uv([x, y, z]: [f64; 3]) -> [f32; 2] {
    let denominator = x + 15.0 * y + 3.0 * z;
    [(4.0 * x / denominator) as f32, (9.0 * y / denominator) as f32]
}

fn white_uv() -> [f32; 2] {
    uv([D65[0], D65[1], 1.0 - D65[0] - D65[1]])
}

fn position([u, v]: [f32; 2]) -> [f32; 2] {
    let white = white_uv();
    [0.5 + (u - white[0]) / (2.0 * RADIUS), 0.5 - (v - white[1]) / (2.0 * RADIUS)]
}

/// CIE XYZ at Y = 1 for an occupied vectorscope bin's normalized position.
/// Inverts the u′v′ equations \[1\] so frontends share the scope's coordinates.
pub fn vectorscope_xyz([x, y]: [f32; 2]) -> [f64; 3] {
    let white = white_uv();
    let u = f64::from(white[0] + (x - 0.5) * (2.0 * RADIUS));
    let v = f64::from(white[1] - (y - 0.5) * (2.0 * RADIUS));
    [9.0 * u / (4.0 * v), 1.0, (12.0 - 3.0 * u - 20.0 * v) / (4.0 * v)]
}

fn vector_counts(pixels: &[[f32; 3]], matrix: &color::Mat3) -> Vec<[u32; 3]> {
    pixels
        .par_iter()
        .with_min_len(16384)
        .fold(
            || vec![[0u32; 3]; SIZE * SIZE],
            |mut counts, pixel| {
                let xyz = color::apply(matrix, pixel.map(|v| f64::from(v.max(0.0))));
                // Black has no chromaticity, so it must not create a neutral spike.
                if xyz[1] > 0.0 {
                    let [x, y] = position(uv(xyz)).map(|v| (v * SIZE as f32) as usize);
                    counts[y.min(SIZE - 1) * SIZE + x.min(SIZE - 1)][0] += 1;
                }
                counts
            },
        )
        .reduce(
            || vec![[0; 3]; SIZE * SIZE],
            |mut counts, partial| {
                for (a, b) in counts.iter_mut().zip(partial) {
                    a[0] += b[0];
                }
                counts
            },
        )
}

fn scope(counts: Vec<[u32; 3]>, axes: ScopeAxes, log: bool) -> Result<Evaluated<()>, String> {
    Ok(Evaluated {
        outputs: (),
        view: Some(View::Scope(Arc::new(Scope { size: SIZE, counts, axes, log }))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_preserves_columns_channels_and_exposure() {
        let image = Rgb {
            width: 2,
            height: 2,
            scale: 1,
            pixels: vec![[1.0, 0.0, 16.0], [0.5; 3], [1.0, -1.0, 32.0], [0.5; 3]],
        };
        let counts = waveform_counts(&image, -12.0, 4.0);
        assert_eq!(counts.iter().flatten().sum::<u32>(), 12);
        assert_eq!(counts[63 * SIZE][0], 2);
        assert_eq!(counts[255 * SIZE][1], 2);
        assert_eq!(counts[0][2], 2);
        assert_eq!(counts[79 * SIZE + 128], [2; 3]);
    }

    #[test]
    fn vectorscope_counts_neutrals_omits_black_and_clips_negative_channels() {
        let matrix = color::rgb_to_xyz(REC2020, D65);
        let mut pixels = vec![[0.18; 3]; 40000];
        pixels.extend([[0.0; 3], [-1.0; 3], [1.0, -0.2, 0.0], [4.0, 0.0, 0.0]]);
        let run = |workers| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| vector_counts(&pixels, &matrix))
        };
        let counts = run(1);
        assert_eq!(counts, run(4));
        assert_eq!(counts.iter().flatten().sum::<u32>(), 40002);
        assert_eq!(counts[128 * SIZE + 128][0], 40000);
        assert_eq!(counts.iter().filter(|c| c[0] == 2).count(), 1);
        assert!(vector_counts(&[], &matrix).iter().all(|c| *c == [0; 3]));
    }

    #[test]
    fn chromaticity_matches_cie_coordinates_and_is_exposure_independent() {
        let matrix = color::rgb_to_xyz(REC2020, D65);
        let white = uv(color::apply(&matrix, [1.0; 3]));
        assert!((white[0] - 0.197830).abs() < 1e-6);
        assert!((white[1] - 0.468320).abs() < 1e-6);
        assert_eq!(position(white), [0.5; 2]);
        let red = uv(color::apply(&matrix, [1.0, 0.0, 0.0]));
        assert!((red[0] - 0.556604).abs() < 1e-6);
        assert!((red[1] - 0.516509).abs() < 1e-6);
        assert_eq!(red, uv(color::apply(&matrix, [4.0, 0.0, 0.0])));
    }
}
