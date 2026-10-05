// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2022-2026 darktable developers.
//! Inpaint opposed, developed by garagecoder and Iain of G'MIC and Hanno
//! Schwalm of darktable \[1\]. Estimate a clipped channel from the cube-root
//! average of the other two; learn a chrominance offset near clipped areas.
//! Drip processes as-shot-balanced Bayer data (no late chromatic adaptation).
//! Unlike upstream, partial mask cells and the last sensor row/column are
//! included, and mask dilation is clipped at image edges. Fixed row reductions
//! accumulated in f64 keep results independent of Rayon's worker count.
//!
//! \[1\] darktable developers, “opposed.c” and “segbased.c,” commit
//! 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/hlreconstruct>
//! See THIRD_PARTY.md for credits and license.

use crate::image::Mosaic;
use rayon::prelude::*;

pub(super) fn process(m: &Mosaic, threshold: f32) -> Vec<f32> {
    if m.data.is_empty() {
        return Vec::new();
    }
    let clips = m.white.map(|v| v * threshold);
    let (mw, mh) = (m.width.div_ceil(3), m.height.div_ceil(3));
    let mask: Vec<[bool; 3]> = (0..mw * mh)
        .into_par_iter()
        .map(|i| {
            let (r, c) = (i / mw * 3, i % mw * 3);
            let mut mask = [false; 3];
            for y in r..(r + 3).min(m.height) {
                for x in c..(c + 3).min(m.width) {
                    let color = m.cfa.color(y, x) as usize;
                    mask[rgb(color)] |= m.data[y * m.width + x] >= clips[color];
                }
            }
            mask
        })
        .collect();
    if !mask.iter().flatten().any(|&v| v) {
        return m.data.clone();
    }
    let near: Vec<[bool; 3]> = (0..mw * mh)
        .into_par_iter()
        .map(|i| {
            let (r, c) = (i / mw, i % mw);
            let mut out = [false; 3];
            for y in r.saturating_sub(3)..=(r + 3).min(mh - 1) {
                for x in c.saturating_sub(3)..=(c + 3).min(mw - 1) {
                    // Upstream's 7×7 footprint omits its four corners.
                    if y.abs_diff(r) == 3 && x.abs_diff(c) == 3 {
                        continue;
                    }
                    for (v, &hit) in out.iter_mut().zip(&mask[y * mw + x]) {
                        *v |= hit;
                    }
                }
            }
            out
        })
        .collect();
    // A fixed reduction per row, then a serial reduction of those rows. Scheduling
    // may change, but neither sample selection nor floating-point grouping does.
    let rows: Vec<_> = (0..m.height)
        .into_par_iter()
        .map(|r| {
            let mut sums = [0.0f64; 3];
            let mut counts = [0u64; 3];
            for c in 0..m.width {
                let color = m.cfa.color(r, c) as usize;
                let channel = rgb(color);
                let value = m.data[r * m.width + c];
                if value > 0.2 * clips[color]
                    && value < clips[color]
                    && near[r / 3 * mw + c / 3][channel]
                {
                    sums[channel] += f64::from(value - reference(m, r, c));
                    counts[channel] += 1;
                }
            }
            (sums, counts)
        })
        .collect();
    let (sum, count) =
        rows.into_iter().fold(([0.0; 3], [0u64; 3]), |(mut s, mut n), (values, counts)| {
            for c in 0..3 {
                s[c] += values[c];
                n[c] += counts[c];
            }
            (s, n)
        });
    let chroma: [f32; 3] =
        std::array::from_fn(
            |c| if count[c] > 100 { (sum[c] / count[c] as f64) as f32 } else { 0.0 },
        );
    m.data
        .par_iter()
        .enumerate()
        .map(|(i, &value)| {
            let (r, c) = (i / m.width, i % m.width);
            let color = m.cfa.color(r, c) as usize;
            if value >= clips[color] {
                value.max(reference(m, r, c) + chroma[rgb(color)])
            } else {
                value
            }
        })
        .collect()
}

fn rgb(color: usize) -> usize {
    [0, 1, 2, 1][color]
}

fn reference(m: &Mosaic, row: usize, col: usize) -> f32 {
    let mut sums = [0.0f32; 3];
    let mut counts = [0u32; 3];
    for r in row.saturating_sub(1)..=(row + 1).min(m.height - 1) {
        for c in col.saturating_sub(1)..=(col + 1).min(m.width - 1) {
            let color = rgb(m.cfa.color(r, c) as usize);
            sums[color] += m.data[r * m.width + c].max(0.0);
            counts[color] += 1;
        }
    }
    let means: [f32; 3] = std::array::from_fn(|c| (sums[c] / counts[c].max(1) as f32).cbrt());
    let color = rgb(m.cfa.color(row, col) as usize);
    let opposite = 0.5 * (means[(color + 1) % 3] + means[(color + 2) % 3]);
    opposite * opposite * opposite
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Camera, Cfa};
    use std::sync::Arc;
    const PHASES: [[u8; 4]; 4] = [[0, 1, 3, 2], [1, 0, 2, 3], [3, 2, 0, 1], [2, 3, 1, 0]];
    fn mosaic(width: usize, height: usize, phase: usize) -> Mosaic {
        let mut m = Mosaic {
            width,
            height,
            scale: 1,
            cfa: Cfa { size: 2, colors: PHASES[phase].to_vec() },
            white: [1.3, 1.0, 1.7, 1.0],
            camera: Arc::new(Camera {
                xyz_to_cam: [[0.0; 3]; 3],
                white_balance: [1.3, 1.0, 1.7, 1.0],
            }),
            data: vec![0.0; width * height],
        };
        for r in 0..height {
            for c in 0..width {
                let ch = m.cfa.color(r, c) as usize;
                let value = if (32..58).contains(&r) && (32..64).contains(&c) {
                    if ch == 0 { 1.0 } else { 0.9 }
                } else {
                    0.35 + ((r * 13 + c * 7) % 31) as f32 / 100.0
                };
                m.data[r * width + c] = value * m.white[ch];
            }
        }
        m
    }
    #[test]
    fn matches_upstream_reconstruction_and_chrominance_estimate() {
        for phase in 0..4 {
            let m = mosaic(96, 90, phase);
            let out = process(&m, 0.98);
            for line in include_str!("../../../tests/reference/opposed.csv")
                .lines()
                .filter(|s| !s.starts_with('#'))
            {
                let v: Vec<f32> = line.split(',').map(|s| s.parse().unwrap()).collect();
                if v[0] as usize != phase {
                    continue;
                }
                let (r, c) = (v[1] as usize, v[2] as usize);
                assert!(
                    (out[r * m.width + c] - v[3]).abs() < 2e-5,
                    "{phase}, {r},{c}: {} != {}",
                    out[r * m.width + c],
                    v[3]
                );
            }
            for (i, (&before, &after)) in m.data.iter().zip(&out).enumerate() {
                let ch = m.cfa.color(i / m.width, i % m.width) as usize;
                if before < m.white[ch] * 0.98 {
                    assert_eq!(before, after);
                } else {
                    assert!(after >= before);
                }
            }
        }
    }
    #[test]
    fn empty_small_partial_cells_and_worker_count() {
        for (w, h) in [(0, 0), (2, 2), (5, 7), (97, 91)] {
            let mut m = mosaic(w, h, 0);
            assert_eq!(process(&m, 1.0).len(), m.data.len());
            for i in (0..m.data.len()).step_by(7) {
                m.data[i] = 3.0;
            }
            let run = |n| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(n)
                    .build()
                    .unwrap()
                    .install(|| process(&m, 0.98))
            };
            let out = run(1);
            assert_eq!(out, run(4));
            assert!(out.iter().all(|v| v.is_finite()));
        }
        let m = mosaic(20, 20, 0);
        assert_eq!(process(&m, 0.98), m.data);
    }
    #[test]
    fn clipped_channel_uses_other_channels_and_keeps_unclipped_samples() {
        let mut m = mosaic(16, 16, 0);
        m.white = [1.0; 4];
        for r in 0..m.height {
            for c in 0..m.width {
                let ch = rgb(m.cfa.color(r, c) as usize);
                m.data[r * m.width + c] = if ch == 0 { 1.0 } else { 2.0 };
            }
        }
        // Only red is classified as clipped; the other channels provide detail.
        m.white = [1.0, 4.0, 4.0, 4.0];
        let out = process(&m, 0.98);
        for r in 0..m.height {
            for c in 0..m.width {
                assert!((out[r * m.width + c] - 2.0).abs() < 2e-6);
            }
        }
    }
}
