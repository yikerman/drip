//! Ordinary Rust kernels sharing Rayon's worker pool; values stay in host memory.

use rayon::prelude::*;

pub const BINS: usize = 256;

pub fn white_balance(input: &[f32], width: usize, cfa_size: usize, gains: &[f32]) -> Vec<f32> {
    input
        .par_iter()
        .enumerate()
        .map(|(i, &value)| {
            let phase = (i / width % cfa_size) * cfa_size + i % width % cfa_size;
            value * gains[phase]
        })
        .collect()
}

pub fn debayer(
    input: &[f32],
    width: usize,
    height: usize,
    cfa_size: usize,
    colors: &[u32],
) -> Vec<[f32; 3]> {
    (0..(width / 2) * (height / 2))
        .into_par_iter()
        .map(|i| {
            let row = i / (width / 2) * 2;
            let col = i % (width / 2) * 2;
            let (mut sum, mut count) = ([0.0; 3], [0.0; 3]);
            for r in 0..2 {
                for c in 0..2 {
                    let phase = ((row + r) % cfa_size) * cfa_size + (col + c) % cfa_size;
                    let color = colors[phase] as usize;
                    sum[color] += input[(row + r) * width + col + c];
                    count[color] += 1.0;
                }
            }
            std::array::from_fn(|c| sum[c] / count[c])
        })
        .collect()
}

pub fn matrix(input: &[[f32; 3]], matrix: &[[f32; 3]; 3]) -> Vec<[f32; 3]> {
    input
        .par_iter()
        .map(|&[r, g, b]| matrix.map(|row| row[0] * r + row[1] * g + row[2] * b))
        .collect()
}

pub fn sigmoid(input: &[[f32; 3]], contrast: f32, offset: f32) -> Vec<[f32; 3]> {
    input
        .par_iter()
        .map(|pixel| {
            pixel.map(|value| {
                // Evaluate x^c / (x^c + k) in log space to avoid overflowing x^c.
                let z = value.max(f32::MIN_POSITIVE).ln() * contrast + offset;
                if value > 0.0 { 1.0 / (1.0 + (-z).clamp(-80.0, 80.0).exp()) } else { 0.0 }
            })
        })
        .collect()
}

pub fn histogram(input: &[[f32; 3]], thresholds: &[f32; BINS]) -> Vec<[u32; 3]> {
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
    fn concurrent_evaluations_keep_their_buffers_independent() {
        std::thread::scope(|scope| {
            for worker in 0..4 {
                scope.spawn(move || {
                    let matrix = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
                    for round in 0..8 {
                        let value = (worker * 8 + round) as f32;
                        let input = vec![[value; 3]; 4097 + round];
                        let output: Vec<[f32; 3]> = super::matrix(&input, &matrix);
                        assert!(
                            output
                                .iter()
                                .all(|&pixel| pixel == [value * 2.0, value * 3.0, value * 4.0])
                        );
                    }
                });
            }
        });
    }

    #[test]
    fn empty_images() {
        assert!(sigmoid(&[], 1.0, 0.0).is_empty());
        assert!(debayer(&[1.0], 1, 1, 2, &[0, 1, 1, 2]).is_empty());
        assert_eq!(histogram(&[], &[0.0; BINS]), vec![[0; 3]; BINS]);
    }

    #[test]
    fn cfa_kernels_cover_phases_and_crop_odd_edges() {
        let (width, height) = (19, 17);
        for colors in [[0, 1, 1, 2], [1, 0, 2, 1], [1, 2, 0, 1], [2, 1, 1, 0]] {
            let gains = [2.0, 1.0, 3.0, 4.0];
            let input: Vec<_> = (0..width * height).map(|i| i as f32 - 100.0).collect();
            let balanced = white_balance(&input, width, 2, &gains);
            for (i, &value) in balanced.iter().enumerate() {
                assert_eq!(value, input[i] * gains[i / width % 2 * 2 + i % width % 2]);
            }
            let pixels = debayer(&balanced, width, height, 2, &colors);
            for (i, pixel) in pixels.iter().enumerate() {
                let origin = i / (width / 2) * 2 * width + i % (width / 2) * 2;
                let mut expected = [0.0; 3];
                for (phase, offset) in [0, 1, width, width + 1].into_iter().enumerate() {
                    expected[colors[phase] as usize] += balanced[origin + offset];
                }
                expected[1] /= 2.0;
                assert_eq!(*pixel, expected);
            }
        }
    }

    #[test]
    fn matrix_matches_f64_reference_at_uneven_lengths() {
        let matrix = [[1.2, -0.1, -0.1], [-0.3, 1.5, -0.2], [0.0, -0.1, 1.1]];
        for len in [1, 17, 4097] {
            let input: Vec<_> = (0..len)
                .map(|i| std::array::from_fn(|c| ((i * 3 + c) * 37 % 1009) as f32 / 71.0 - 2.0))
                .collect();
            let output: Vec<[f32; 3]> = super::matrix(&input, &matrix);
            for (pixel, source) in output.iter().zip(&input) {
                for (value, row) in pixel.iter().zip(matrix) {
                    let expected: f64 =
                        row.iter().zip(source).map(|(&a, &b)| f64::from(a) * f64::from(b)).sum();
                    assert!((f64::from(*value) - expected).abs() < 4e-6);
                }
            }
        }
    }

    #[test]
    fn sigmoid_matches_f64_curve_across_parameter_range_and_uneven_lengths() {
        let mut input: Vec<_> = (0..512).map(|i| 2f32.powf(i as f32 / 8.0 - 32.0)).collect();
        input[..8].copy_from_slice(&[
            -1.0,
            0.0,
            f32::from_bits(1),
            f32::MIN_POSITIVE,
            0.09,
            0.18,
            1e30,
            f32::MAX,
        ]);
        for exposure in [-10.0f32, -3.0, 0.0, 3.0, 10.0] {
            for contrast in [0.5f32, 1.0, 1.5, 2.5, 4.0] {
                let k = 0.18f32.powf(contrast) * (1.0 / 0.18 - 1.0);
                let offset = contrast * exposure * std::f32::consts::LN_2 - k.ln();
                for len in [1, 7, 510, 512] {
                    let pixels: Vec<_> = input[..len].iter().map(|&v| [v; 3]).collect();
                    let output = sigmoid(&pixels, contrast, offset);
                    for (pixel, &source) in output.iter().zip(&input) {
                        assert_eq!(pixel[0], pixel[1]);
                        assert_eq!(pixel[1], pixel[2]);
                        let value = pixel[0];
                        let x = (f64::from(source) * 2f64.powf(f64::from(exposure)))
                            .max(0.0)
                            .powf(f64::from(contrast));
                        let k = 0.18f64.powf(f64::from(contrast)) * (1.0 / 0.18 - 1.0);
                        let expected = x / (x + k);
                        assert!(
                            (f64::from(value) - expected).abs() < 2e-6,
                            "{source}, {exposure}, {contrast}: {value} != {expected}"
                        );
                    }
                }
            }
        }
    }

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
        let counts = histogram(&input, &thresholds);
        assert_eq!(counts, expected);
    }
}
