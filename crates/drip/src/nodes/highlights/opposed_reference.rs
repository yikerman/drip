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
    if m.samples().is_empty() {
        return Vec::new();
    }
    let clips = m.interpretation().white.map(|v| v * threshold);
    let (mw, mh) = (m.width.div_ceil(3), m.height.div_ceil(3));
    let mask: Vec<[bool; 3]> = (0..mw * mh)
        .into_par_iter()
        .map(|i| {
            let (r, c) = (i / mw * 3, i % mw * 3);
            let mut mask = [false; 3];
            for y in r..(r + 3).min(m.height) {
                for x in c..(c + 3).min(m.width) {
                    let color = m.interpretation().cfa.color(y, x) as usize;
                    mask[rgb(color)] |= m.samples()[y * m.width + x] >= clips[color];
                }
            }
            mask
        })
        .collect();
    if !mask.iter().flatten().any(|&v| v) {
        return m.samples().to_vec();
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
                let color = m.interpretation().cfa.color(r, c) as usize;
                let channel = rgb(color);
                let value = m.samples()[r * m.width + c];
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
    m.samples()
        .par_iter()
        .enumerate()
        .map(|(i, &value)| {
            let (r, c) = (i / m.width, i % m.width);
            let color = m.interpretation().cfa.color(r, c) as usize;
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
            let color = rgb(m.interpretation().cfa.color(r, c) as usize);
            sums[color] += m.samples()[r * m.width + c].max(0.0);
            counts[color] += 1;
        }
    }
    let means: [f32; 3] = std::array::from_fn(|c| (sums[c] / counts[c].max(1) as f32).cbrt());
    let color = rgb(m.interpretation().cfa.color(row, col) as usize);
    let opposite = 0.5 * (means[(color + 1) % 3] + means[(color + 2) % 3]);
    opposite * opposite * opposite
}
