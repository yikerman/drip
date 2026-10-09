// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) darktable developers.
// Source pins and numerical deviations: THIRD_PARTY.md.
use crate::node::shared_kernel::color;
use cubecl::prelude::*;
#[cube]
fn sample(a: &[f32], i: usize, d: i32) -> f32 {
    {
        let j = (i as i32 + d) as usize;
        let v: f32 = a[j];
        v.max(0.0f32)
    }
}
#[cube]
fn high(a: &[f32], i: usize, d: i32) -> f32 {
    let v = (sample(a, i, -3i32 * d) - sample(a, i, -d) - sample(a, i, d) + sample(a, i, 3 * d))
        - 3.0f32 * (sample(a, i, -2i32 * d) + sample(a, i, 2 * d))
        + 6.0f32 * sample(a, i, 0);
    v * v
}
#[cube]
fn direction(map: &[f32], i: usize, w: usize) -> f32 {
    let neighbor = 0.25f32 * (map[i - w - 1] + map[i - w + 1] + map[i + w - 1] + map[i + w + 1]);
    if (0.5f32 - map[i]).abs() < (0.5f32 - neighbor).abs() { neighbor } else { map[i] }
}
#[cube]
fn blend(t: f32, a: f32, b: f32) -> f32 {
    let t = t.clamp(0.0f32, 1.0f32);
    t * a + (1.0f32 - t) * b
}
#[cube]
fn estimate_green(a: &[f32], low: &[f32], i: usize, d: i32) -> (f32, f32) {
    let grad = 1e-5f32
        + (sample(a, i, d) - sample(a, i, -d)).abs()
        + (sample(a, i, 0) - sample(a, i, 2 * d)).abs()
        + (sample(a, i, d) - sample(a, i, 3 * d)).abs()
        + (sample(a, i, 2 * d) - sample(a, i, 4 * d)).abs();
    let value =
        sample(a, i, d) * (low[i] + low[i]) / (1e-5f32 + low[i] + low[(i as i32 + 2 * d) as usize]);
    (grad, value)
}
#[cube]
fn weighted(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 * b.1 + b.0 * a.1) / (a.0 + b.0)
}
#[cube]
fn rgb_at(a: &[f32], i: usize, d: i32, c: usize) -> f32 {
    a[((i as i32 + d) as usize) * 3 + c]
}
#[cube]
fn estimate_rgb(a: &[f32], i: usize, d: i32, c: usize) -> (f32, f32) {
    let grad = 1e-5f32
        + (rgb_at(a, i, d, c) - rgb_at(a, i, -d, c)).abs()
        + (rgb_at(a, i, d, c) - rgb_at(a, i, 3 * d, c)).abs()
        + (rgb_at(a, i, 0, 1) - rgb_at(a, i, 2 * d, 1)).abs();
    (grad, rgb_at(a, i, d, c) - rgb_at(a, i, d, 1))
}
#[cube(launch)]
pub fn rcd_maps(
    a: &[f32],
    vh: &mut [f32],
    low: &mut [f32],
    p: &mut [f32],
    q: &mut [f32],
    rgb: &mut [f32],
    w: usize,
    h: usize,
    phase: usize,
) {
    let i = ABSOLUTE_POS;
    if i < a.len() {
        let r = i / w;
        let c = i % w;
        let d = w as i32;
        vh[i] = 0.0f32;
        low[i] = 0.0f32;
        p[i] = 0.0f32;
        q[i] = 0.0f32;
        for ch in 0..3 {
            rgb[i * 3 + ch] = if ch == color(r, 0, phase) || ch == color(r, 1, phase) {
                a[i].max(0.0f32)
            } else {
                0.0f32
            };
        }
        if r >= 4 && c >= 4 && r + 4 < h && c + 4 < w {
            let v = (high(a, i - w, d) + high(a, i, d) + high(a, i + w, d)).max(1e-10f32);
            let hh = (high(a, i, 1) + high(a, i - 1, 1) + high(a, i + 1, 1)).max(1e-10f32);
            vh[i] = v / (v + hh);
        }
        if r >= 2 && c >= 2 && r + 2 < h && c + 2 < w && color(r, c, phase) != 1 {
            low[i] = sample(a, i, 0)
                + 0.5f32
                    * (sample(a, i, -d) + sample(a, i, d) + sample(a, i, -1) + sample(a, i, 1))
                + 0.25f32
                    * (sample(a, i, -d - 1)
                        + sample(a, i, -d + 1)
                        + sample(a, i, d - 1)
                        + sample(a, i, d + 1));
        }
        if r >= 3 && c >= 3 && r + 3 < h && c + 3 < w && c % 2 == 1 {
            p[i] = high(a, i, d + 1);
            q[i] = high(a, i, d - 1);
        }
    }
}
#[cube(launch)]
pub fn rcd_green(
    a: &[f32],
    vh: &[f32],
    low: &[f32],
    rgb: &mut [f32],
    w: usize,
    h: usize,
    phase: usize,
) {
    let i = ABSOLUTE_POS;
    if i < a.len() {
        let r = i / w;
        let c = i % w;
        if r >= 4 && c >= 4 && r + 4 < h && c + 4 < w && color(r, c, phase) != 1 {
            let d = w as i32;
            let v = weighted(estimate_green(a, low, i, -d), estimate_green(a, low, i, d));
            let hh = weighted(estimate_green(a, low, i, -1), estimate_green(a, low, i, 1));
            rgb[i * 3 + 1] = blend(direction(vh, i, w), hh, v);
        }
    }
}
#[cube(launch)]
pub fn rcd_pq(p: &[f32], q: &[f32], pq: &mut [f32], w: usize, h: usize, phase: usize) {
    let i = ABSOLUTE_POS;
    if i < pq.len() {
        let r = i / w;
        let c = i % w;
        pq[i] = 0.0f32;
        if r >= 4 && c >= 4 && r + 4 < h && c + 4 < w && color(r, c, phase) != 1 {
            let a = (r - 1) * w + ((c - 1) | 1);
            let b = r * w + (c | 1);
            let d = (r + 1) * w + ((c - 1) | 1);
            let ps = (p[a] + p[b] + p[d + 2]).max(1e-10f32);
            let qs = (q[a + 2] + q[b] + q[d]).max(1e-10f32);
            pq[i] = ps / (ps + qs);
        }
    }
}
#[cube(launch)]
pub fn rcd_opposite(rgb: &[f32], pq: &[f32], out: &mut [f32], w: usize, h: usize, phase: usize) {
    let i = ABSOLUTE_POS;
    if i < pq.len() {
        let r = i / w;
        let c = i % w;
        for ch in 0..3 {
            out[i * 3 + ch] = rgb[i * 3 + ch];
        }
        if r >= 4 && c >= 4 && r + 4 < h && c + 4 < w && color(r, c, phase) != 1 {
            let ch = 2 - color(r, c, phase);
            let d = w as i32;
            let v = blend(
                direction(pq, i, w),
                weighted(estimate_rgb(rgb, i, -d + 1, ch), estimate_rgb(rgb, i, d - 1, ch)),
                weighted(estimate_rgb(rgb, i, -d - 1, ch), estimate_rgb(rgb, i, d + 1, ch)),
            );
            out[i * 3 + ch] = rgb[i * 3 + 1] + v;
        }
    }
}
#[cube(launch)]
pub fn rcd_finish(
    a: &[f32],
    rgb: &[f32],
    vh: &[f32],
    out: &mut [f32],
    w: usize,
    h: usize,
    phase: usize,
) {
    let i = ABSOLUTE_POS;
    if i < a.len() {
        let r = i / w;
        let c = i % w;
        if r >= 10 && c >= 10 && r + 10 < h && c + 10 < w {
            for ch in 0..3 {
                out[i * 3 + ch] = rgb[i * 3 + ch].max(0.0f32);
            }
            if color(r, c, phase) == 1 {
                for k in 0..2 {
                    let ch = k * 2;
                    let d = w as i32;
                    let v = blend(
                        direction(vh, i, w),
                        weighted(estimate_rgb(rgb, i, -1, ch), estimate_rgb(rgb, i, 1, ch)),
                        weighted(estimate_rgb(rgb, i, -d, ch), estimate_rgb(rgb, i, d, ch)),
                    );
                    out[i * 3 + ch] = (rgb[i * 3 + 1] + v).max(0.0f32);
                }
            }
        } else {
            let mut sum = Array::<f32>::new(3usize);
            let mut count = Array::<f32>::new(3usize);
            for ch in 0..3 {
                sum[ch] = 0.0f32;
                count[ch] = 0.0f32;
            }
            for yy in 0..3 {
                for xx in 0..3 {
                    let y = r as i32 + yy - 1;
                    let x = c as i32 + xx - 1;
                    if y >= 0 && x >= 0 && y < h as i32 && x < w as i32 {
                        let ch = color(y as usize, x as usize, phase);
                        sum[ch] += a[y as usize * w + x as usize].max(0.0f32);
                        count[ch] += 1.0f32;
                    }
                }
            }
            for ch in 0..3 {
                out[i * 3 + ch] = sum[ch] / count[ch].max(1.0f32);
            }
            out[i * 3 + color(r, c, phase)] = a[i].max(0.0f32);
        }
    }
}
