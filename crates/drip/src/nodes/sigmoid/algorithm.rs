//! Per-channel curve, independent of graph evaluation.

use rayon::prelude::*;

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn empty_image() {
        assert!(sigmoid(&[], 1.0, 0.0).is_empty());
    }
}
