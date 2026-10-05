// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2020-2026 darktable developers.
//! Per-channel sigmoid and hue/energy correction adapted from darktable \[1\].
//! Fixed Rec.2020 primaries use its smooth preset's attenuation and rotation;
//! purity recovery is zero. Drip retains its 0.18 grey point and zero black.
//! Coefficients use analytic slopes and the curve uses log space to avoid
//! overflowing powers; neither changes the underlying log-logistic curve.
//!
//! \[1\] darktable developers, “sigmoid.c” and “custom_primaries.c,” commit
//! 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3, 2026. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src>
//! License: GPL-3.0-or-later; see THIRD_PARTY.md for credits and the upstream license.

use crate::color::{self, D65, REC2020};
use rayon::prelude::*;

pub const GREY: f32 = 0.18;

/// Prepared once for processing or sampling the same curve in a frontend.
/// Callers supply schema-validated contrast, skew and hue preservation.
pub struct Sigmoid {
    film: f32,
    paper: f32,
    log_exposure: f32,
    hue: f32,
    inset: [[f32; 3]; 3],
    outset: [[f32; 3]; 3],
}

impl Sigmoid {
    pub fn new(contrast: f32, skew: f32, hue: f32) -> Self {
        let grey = f64::from(GREY);
        let paper = 5.0f64.powf(-f64::from(skew));
        let grey_root = grey.powf(1.0 / paper);
        // Match the unskewed curve's derivative at grey: c * (1-grey).
        let film = f64::from(contrast) * (1.0 - grey) / (paper * (1.0 - grey_root));
        let log_exposure = film * grey.ln() + (1.0 / grey_root - 1.0).ln();
        let base = color::rgb_to_xyz(REC2020, D65);
        let to_base = color::inverse(&base);
        let inset = color::mul(&to_base, &color::rgb_to_xyz(primaries([0.9, 0.9, 0.85]), D65));
        let outset =
            color::inverse(&color::mul(&to_base, &color::rgb_to_xyz(primaries([1.0; 3]), D65)));
        Self {
            film: film as f32,
            paper: paper as f32,
            log_exposure: log_exposure as f32,
            hue,
            inset: color::to_f32(&inset),
            outset: color::to_f32(&outset),
        }
    }

    pub fn curve(&self, value: f32) -> f32 {
        if value <= 0.0 {
            return 0.0;
        }
        let z = self.log_exposure - self.film * value.ln();
        // log(1 + exp(z)), without overflow or loss of tiny positive tails.
        let softplus = z.max(0.0) + (-z.abs()).exp().ln_1p();
        (-self.paper * softplus).exp()
    }

    pub fn pixel(&self, input: [f32; 3]) -> [f32; 3] {
        let input = apply(&self.inset, positive(input));
        let mapped = input.map(|v| self.curve(v));
        apply(&self.outset, preserve_hue(input, mapped, self.hue))
    }

    pub fn process(&self, input: &[[f32; 3]]) -> Vec<[f32; 3]> {
        input.par_iter().map(|&p| self.pixel(p)).collect()
    }
}

fn apply(matrix: &[[f32; 3]; 3], pixel: [f32; 3]) -> [f32; 3] {
    matrix.map(|r| r[0] * pixel[0] + r[1] * pixel[1] + r[2] * pixel[2])
}

/// Move negative channels toward the achromatic axis, preserving the average.
fn positive(pixel: [f32; 3]) -> [f32; 3] {
    // Divide before summing so finite HDR inputs do not overflow their average.
    let average = pixel.iter().map(|v| v / 3.0).sum::<f32>().max(0.0);
    let min = pixel.into_iter().fold(f32::INFINITY, f32::min);
    let saturation = if min < 0.0 {
        (f64::from(average) / (f64::from(average) - f64::from(min))) as f32
    } else {
        1.0
    };
    pixel.map(|v| ((1.0 - saturation) * average + saturation * v).max(0.0))
}

fn preserve_hue(input: [f32; 3], mapped: [f32; 3], hue: f32) -> [f32; 3] {
    let mut order = [0, 1, 2];
    order.sort_by(|&a, &b| input[a].total_cmp(&input[b]));
    let [lo, mid, hi] = order;
    let chroma = input[hi] - input[lo];
    let fraction = if chroma != 0.0 { (input[mid] - input[lo]) / chroma } else { 0.0 };
    let corrected = mapped[lo] + (mapped[hi] - mapped[lo]) * fraction;
    let naive = (1.0 - hue) * mapped[mid] + hue * corrected;
    let sum = f64::from(input[lo]) + f64::from(input[mid]);
    let blend = if sum != 0.0 { (2.0 * f64::from(input[lo]) / sum) as f32 } else { 0.0 };
    let energy =
        blend * mapped.iter().sum::<f32>() + (1.0 - blend) * (mapped[lo] + naive + mapped[hi]);
    let mut output = mapped;
    if naive <= mapped[mid] {
        output[mid] = ((1.0 - hue) * mapped[mid]
            + hue * (fraction * mapped[hi] + (1.0 - fraction) * (energy - mapped[hi])))
            / (1.0 + hue * (1.0 - fraction));
        output[lo] = energy - mapped[hi] - output[mid];
    } else {
        output[mid] = ((1.0 - hue) * mapped[mid]
            + hue * (mapped[lo] * (1.0 - fraction) + fraction * (energy - mapped[lo])))
            / (1.0 + hue * fraction);
        output[hi] = energy - mapped[lo] - output[mid];
    }
    output
}

/// Rotate each spectral direction to the base triangle's edge, then attenuate.
/// This follows darktable's ray/edge construction, not rotation about the origin.
fn primaries(scale: [f64; 3]) -> [[f64; 2]; 3] {
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    std::array::from_fn(|i| {
        let d = sub(REC2020[i], D65);
        let angle = d[1].atan2(d[0]) + [2.0f64, -1.0, -3.0][i].to_radians();
        let dir = [angle.cos(), angle.sin()];
        let distance = (0..3)
            .filter_map(|j| {
                let edge = sub(REC2020[(j + 1) % 3], REC2020[j]);
                let t = cross(sub(REC2020[j], D65), edge) / cross(dir, edge);
                (t >= 0.0).then_some(t)
            })
            .fold(f64::INFINITY, f64::min);
        [D65[0] + scale[i] * distance * dir[0], D65[1] + scale[i] * distance * dir[1]]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_matches_f64_definition_and_preserves_grey_slope() {
        for contrast in [0.5, 1.0, 1.5, 4.0] {
            for skew in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let s = Sigmoid::new(contrast, skew, 1.0);
                assert!((s.curve(GREY) - GREY).abs() < 2e-6);
                let slope = (s.curve(GREY + 0.0001) - s.curve(GREY - 0.0001)) / 0.0002;
                assert!((slope - contrast * (1.0 - GREY)).abs() < 0.003);
                let q = 5.0f64.powf(-f64::from(skew));
                let g = f64::from(GREY);
                let p = f64::from(contrast) * (1.0 - g) / (q * (1.0 - g.powf(1.0 / q)));
                let exposure = g.powf(p) * (g.powf(-1.0 / q) - 1.0);
                for x in [0.0f32, f32::from_bits(1), 0.001, 0.09, GREY, 1.0, 100.0, f32::MAX] {
                    let expected = (1.0 + exposure * f64::from(x).powf(-p)).powf(-q);
                    assert!((f64::from(s.curve(x)) - expected).abs() < 2e-6);
                }
            }
        }
    }

    #[test]
    fn colour_handles_neutrals_negatives_and_extremes() {
        for hue in [0.0, 0.5, 1.0] {
            let s = Sigmoid::new(1.5, -0.2, hue);
            for v in [0.0, GREY, 1.0, 1e10, f32::MAX] {
                let pixel = s.pixel([v; 3]);
                for c in pixel {
                    assert!((c - s.curve(v)).abs() < 3e-6, "{v}: {pixel:?}");
                }
            }
            for p in [[-1.0; 3], [-0.1, 0.2, 1.0], [100.0, 0.1, 0.0], [f32::MAX, 0.0, 1.0]] {
                assert!(s.pixel(p).iter().all(|v| v.is_finite()));
            }
            assert!(s.process(&[]).is_empty());
        }
    }

    #[test]
    fn hue_correction_matches_upstream_c_vectors() {
        for line in include_str!("../../../tests/reference/sigmoid_hue.csv")
            .lines()
            .filter(|s| !s.starts_with('#'))
        {
            let v: Vec<f32> = line.split(',').map(|s| s.parse().unwrap()).collect();
            let output =
                preserve_hue(v[0..3].try_into().unwrap(), v[3..6].try_into().unwrap(), v[6]);
            for c in 0..3 {
                assert!((output[c] - v[7 + c]).abs() < 2e-6);
            }
        }
    }

    #[test]
    fn hue_preservation_and_rayon_are_consistent() {
        let s = Sigmoid::new(1.5, 0.0, 1.0);
        let input: Vec<_> = (0..4099).map(|i| [i as f32 / 1024.0, 0.18, 0.5]).collect();
        for workers in [1, 4] {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(workers).build().unwrap();
            assert_eq!(
                pool.install(|| s.process(&input)),
                input.iter().map(|&p| s.pixel(p)).collect::<Vec<_>>()
            );
        }
        let p = [0.1, 0.4, 2.0];
        let out = preserve_hue(p, p.map(|v| s.curve(v)), 1.0);
        assert!(
            ((out[1] - out[0]) / (out[2] - out[0]) - (p[1] - p[0]) / (p[2] - p[0])).abs() < 1e-6
        );
    }
}
