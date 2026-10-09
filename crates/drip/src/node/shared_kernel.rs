// SPDX-License-Identifier: GPL-3.0-or-later
// Source pins and numerical deviations: THIRD_PARTY.md.
use super::data::Extent;
use cubecl::prelude::*;

#[cube(launch)]
pub fn matrix(input: &[f32], coeff: &[f32], output: &mut [f32]) {
    let i = ABSOLUTE_POS * 3;
    if i < input.len() {
        let r = input[i];
        let g = input[i + 1];
        let b = input[i + 2];
        for c in 0..3 {
            output[i + c] = coeff[c * 3] * r + coeff[c * 3 + 1] * g + coeff[c * 3 + 2] * b;
        }
    }
}
#[cube]
pub fn color(row: usize, col: usize, phase: usize) -> usize {
    let ry = (row % 2) ^ (phase / 2);
    let cx = (col % 2) ^ (phase % 2);
    if ry == cx { ry * 2 } else { 1usize }
}
// Exact exponent shift prevents driver reassociation from recreating an overflowing sum.
#[cube]
pub(crate) fn down64(v: f32) -> f32 {
    let bits = u32::reinterpret(v);
    if ((bits >> 23) & 255) > 64 {
        f32::reinterpret(bits - (64u32 << 23))
    } else {
        v * 5.421_011e-20_f32
    }
}
// Average each Bayer phase plane without mixing CFA sites. This single kernel
// is used by explicit reduction and context-driven demosaic on every backend.
#[cube(launch)]
pub fn reduce_bayer(
    input: &[f32],
    output: &mut [f32],
    width: usize,
    height: usize,
    out_width: usize,
    factor: usize,
) {
    let i = ABSOLUTE_POS;
    if i < output.len() {
        let row = i / out_width;
        let col = i % out_width;
        let y = row / 2 * (2 * factor) + row % 2;
        let x = col / 2 * (2 * factor) + col % 2;
        let mut magnitude = 0.0f32;
        let mut count = 0usize;
        for dy in 0..factor {
            let r = y + 2 * dy;
            for dx in 0..factor {
                let c = x + 2 * dx;
                if r < height && c < width {
                    magnitude = magnitude.max(input[r * width + c].abs());
                    count += 1;
                }
            }
        }
        // Large finite samples need headroom for at most 256² additions.
        // Use an exact exponent shift: dividing by MAX can flush its reciprocal
        // to zero on GPU, and ordinary multiplication can be reassociated.
        let rescale = magnitude > 1.844_674_4e19_f32 && magnitude <= f32::MAX;
        let mut sum = 0.0f32;
        for dy in 0..factor {
            let r = y + 2 * dy;
            for dx in 0..factor {
                let c = x + 2 * dx;
                if r < height && c < width {
                    let value = input[r * width + c];
                    sum += if rescale { down64(value) } else { value };
                }
            }
        }
        let mean = sum / count as f32;
        output[i] = if rescale {
            let bound = down64(magnitude);
            mean.clamp(-bound, bound) * 1.844_674_4e19_f32
        } else {
            mean
        };
    }
}

pub(crate) fn reduce_rgb(
    factor: usize,
    input: &Extent,
    pixels: &[[f32; 3]],
    output: &Extent,
    result: &mut [[f32; 3]],
) {
    let (width, height) = (input.width as usize, input.height as usize);
    for (i, pixel) in result.iter_mut().enumerate() {
        let (x, y) = (i % output.width as usize * factor, i / output.width as usize * factor);
        let (end_x, end_y) = ((x + factor).min(width), (y + factor).min(height));
        let mut sum = [0.0f64; 3];
        for row in y..end_y {
            for sample in &pixels[row * width + x..row * width + end_x] {
                for c in 0..3 {
                    sum[c] += f64::from(sample[c]);
                }
            }
        }
        *pixel = sum.map(|v| (v / ((end_x - x) * (end_y - y)) as f64) as f32);
    }
}
