// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2010-2026 darktable developers.
//! Ratio Corrected Demosaicing by Luis Sanz Rodríguez; tiling by Ingo Weyrich,
//! with optimizations by Luis Sanz Rodríguez and Hanno Schwalm \[1\].
//! Rust uses owned tile scratch and Rayon over disjoint output strips. The
//! ten-pixel halo is essential: later passes depend on earlier interpolations.
//! Unlike darktable's PPG border, the outer ten pixels use bilinear interpolation
//! preserving measured samples. This also defines tiny-image behavior.
//!
//! \[1\] L. Sanz Rodríguez et al., “Ratio Corrected Demosaicing,” darktable
//! `src/iop/demosaicing/rcd.c`, commit 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3.
//! <https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/demosaicing/rcd.c>
//! Original RCD: <https://github.com/LuisSR/RCD-Demosaicing>. See THIRD_PARTY.md.

use crate::image::Mosaic;
use rayon::prelude::*;

const SIDE: usize = 128;
const BORDER: usize = 10;
const STRIDE: usize = SIDE + 2 * BORDER;
const EPS: f32 = 1e-5;

pub(super) fn process(m: &Mosaic) -> Vec<[f32; 3]> {
    if m.samples().is_empty() {
        return Vec::new();
    }
    let mut output = vec![[0.0; 3]; m.samples().len()];
    output.par_chunks_mut(m.width * SIDE).enumerate().for_each(|(band, rows)| {
        let y = band * SIDE;
        for x in (0..m.width).step_by(SIDE) {
            let mut tile = Tile::new(m, x, y);
            tile.interpolate();
            let xmax = (x + SIDE).min(m.width);
            let ymax = (y + SIDE).min(m.height);
            for row in y..ymax {
                for col in x..xmax {
                    let pixel = if row >= BORDER
                        && col >= BORDER
                        && row + BORDER < m.height
                        && col + BORDER < m.width
                    {
                        tile.rgb[(row - tile.y) * STRIDE + col - tile.x].map(|v| v.max(0.0))
                    } else {
                        bilinear(m, row, col)
                    };
                    rows[(row - y) * m.width + col] = pixel;
                }
            }
        }
    });
    output
}

fn color(m: &Mosaic, row: usize, col: usize) -> usize {
    [0, 1, 2, 1][m.interpretation().cfa.color(row, col) as usize]
}

fn bilinear(m: &Mosaic, row: usize, col: usize) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut count = [0; 3];
    for r in row.saturating_sub(1)..=(row + 1).min(m.height - 1) {
        for c in col.saturating_sub(1)..=(col + 1).min(m.width - 1) {
            let channel = color(m, r, c);
            sum[channel] += m.samples()[r * m.width + c].max(0.0);
            count[channel] += 1;
        }
    }
    let mut pixel = std::array::from_fn(|c| sum[c] / count[c].max(1) as f32);
    pixel[color(m, row, col)] = m.samples()[row * m.width + col].max(0.0);
    pixel
}

struct Tile {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    colors: [usize; 4],
    cfa: Vec<f32>,
    rgb: Vec<[f32; 3]>,
}

impl Tile {
    fn new(m: &Mosaic, x: usize, y: usize) -> Self {
        let (left, top) = (x.saturating_sub(BORDER), y.saturating_sub(BORDER));
        let width = (x + SIDE + BORDER).min(m.width) - left;
        let height = (y + SIDE + BORDER).min(m.height) - top;
        let colors = std::array::from_fn(|i| color(m, top + i / 2, left + i % 2));
        let mut tile = Self {
            x: left,
            y: top,
            width,
            height,
            colors,
            cfa: vec![0.0; STRIDE * STRIDE],
            rgb: vec![[0.0; 3]; STRIDE * STRIDE],
        };
        for r in 0..height {
            for c in 0..width {
                let v = m.samples()[(top + r) * m.width + left + c].max(0.0);
                let i = r * STRIDE + c;
                tile.cfa[i] = v;
                // Upstream initializes both colors present in this sensor row.
                tile.rgb[i][colors[r % 2 * 2]] = v;
                tile.rgb[i][colors[r % 2 * 2 + 1]] = v;
            }
        }
        tile
    }

    fn interpolate(&mut self) {
        if self.width <= 2 * BORDER || self.height <= 2 * BORDER {
            return;
        }
        let w = STRIDE as isize;
        let size = STRIDE * STRIDE;
        let mut vh = vec![0.0; size];
        let mut low = vec![0.0; size];
        let cfa = &self.cfa;
        let at = |i: usize, d: isize| cfa[i.wrapping_add_signed(d)];
        let high = |i: usize, d: isize| {
            let v = (at(i, -3 * d) - at(i, -d) - at(i, d) + at(i, 3 * d))
                - 3.0 * (at(i, -2 * d) + at(i, 2 * d))
                + 6.0 * at(i, 0);
            v * v
        };
        // Direction from high-frequency energy in vertical/horizontal neighborhoods.
        for r in 4..self.height - 4 {
            for c in 4..self.width - 4 {
                let i = r * STRIDE + c;
                let v = (high(i - STRIDE, w) + high(i, w) + high(i + STRIDE, w)).max(EPS * EPS);
                let h = (high(i, 1) + high(i - 1, 1) + high(i + 1, 1)).max(EPS * EPS);
                vh[i] = v / (v + h);
            }
        }
        for r in 2..self.height - 2 {
            for c in (2 + (self.colors[r % 2 * 2] & 1)..self.width - 2).step_by(2) {
                let i = r * STRIDE + c;
                low[i] = at(i, 0)
                    + 0.5 * (at(i, -w) + at(i, w) + at(i, -1) + at(i, 1))
                    + 0.25 * (at(i, -w - 1) + at(i, -w + 1) + at(i, w - 1) + at(i, w + 1));
            }
        }
        let direction = |map: &[f32], i: usize| {
            let neighbor = 0.25
                * (map[i - STRIDE - 1]
                    + map[i - STRIDE + 1]
                    + map[i + STRIDE - 1]
                    + map[i + STRIDE + 1]);
            if (0.5 - map[i]).abs() < (0.5 - neighbor).abs() { neighbor } else { map[i] }
        };
        // Green at red/blue sites: directional estimates corrected by local ratios.
        for r in 4..self.height - 4 {
            for c in (4 + (self.colors[r % 2 * 2] & 1)..self.width - 4).step_by(2) {
                let i = r * STRIDE + c;
                let estimate = |d: isize| {
                    let grad = EPS
                        + (at(i, d) - at(i, -d)).abs()
                        + (at(i, 0) - at(i, 2 * d)).abs()
                        + (at(i, d) - at(i, 3 * d)).abs()
                        + (at(i, 2 * d) - at(i, 4 * d)).abs();
                    let value = at(i, d) * (low[i] + low[i])
                        / (EPS + low[i] + low[i.wrapping_add_signed(2 * d)]);
                    (grad, value)
                };
                let vertical = weighted(estimate(-w), estimate(w));
                let horizontal = weighted(estimate(-1), estimate(1));
                self.rgb[i][1] = mix(direction(&vh, i), horizontal, vertical);
            }
        }
        // Diagonal discrimination uses the same packed odd-column samples as RCD.
        let mut p = vec![0.0; size];
        let mut q = vec![0.0; size];
        for r in 3..self.height - 3 {
            for c in (3..self.width - 3).step_by(2) {
                let i = r * STRIDE + c;
                p[i] = high(i, w + 1);
                q[i] = high(i, w - 1);
            }
        }
        let mut pq = vec![0.0; size];
        for r in 4..self.height - 4 {
            for c in (4 + (self.colors[r % 2 * 2] & 1)..self.width - 4).step_by(2) {
                let i = r * STRIDE + c;
                let a = (i - STRIDE - 1) | 1;
                let b = i | 1;
                let d = (i + STRIDE - 1) | 1;
                let ps = (p[a] + p[b] + p[d + 2]).max(EPS * EPS);
                let qs = (q[a + 2] + q[b] + q[d]).max(EPS * EPS);
                pq[i] = ps / (ps + qs);
            }
        }
        // Opposite red/blue: interpolate color differences along the diagonals.
        for r in 4..self.height - 4 {
            for c in (4 + (self.colors[r % 2 * 2] & 1)..self.width - 4).step_by(2) {
                let i = r * STRIDE + c;
                let channel = 2 - self.colors[r % 2 * 2 + c % 2];
                let rgb = &self.rgb;
                let estimate = |d: isize| {
                    let p = |delta: isize, ch: usize| rgb[i.wrapping_add_signed(delta)][ch];
                    let grad = EPS
                        + (p(d, channel) - p(-d, channel)).abs()
                        + (p(d, channel) - p(3 * d, channel)).abs()
                        + (p(0, 1) - p(2 * d, 1)).abs();
                    (grad, p(d, channel) - p(d, 1))
                };
                let disc = direction(&pq, i);
                let value = mix(
                    disc,
                    weighted(estimate(-w + 1), estimate(w - 1)),
                    weighted(estimate(-w - 1), estimate(w + 1)),
                );
                self.rgb[i][channel] = self.rgb[i][1] + value;
            }
        }
        // Red and blue at green sites, using the now-complete neighboring channels.
        for r in 4..self.height - 4 {
            for c in (4 + (self.colors[r % 2 * 2 + 1] & 1)..self.width - 4).step_by(2) {
                let i = r * STRIDE + c;
                for channel in [0, 2] {
                    let rgb = &self.rgb;
                    let estimate = |d: isize| {
                        let p = |delta: isize, ch: usize| rgb[i.wrapping_add_signed(delta)][ch];
                        let grad = EPS
                            + (p(0, 1) - p(2 * d, 1)).abs()
                            + (p(d, channel) - p(-d, channel)).abs()
                            + (p(d, channel) - p(3 * d, channel)).abs();
                        (grad, p(d, channel) - p(d, 1))
                    };
                    let value = mix(
                        direction(&vh, i),
                        weighted(estimate(-1), estimate(1)),
                        weighted(estimate(-w), estimate(w)),
                    );
                    self.rgb[i][channel] = self.rgb[i][1] + value;
                }
            }
        }
    }
}

fn weighted(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 * b.1 + b.0 * a.1) / (a.0 + b.0)
}
fn mix(t: f32, a: f32, b: f32) -> f32 {
    t.clamp(0.0, 1.0) * a + (1.0 - t.clamp(0.0, 1.0)) * b
}
