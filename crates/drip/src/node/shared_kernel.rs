// SPDX-License-Identifier: GPL-3.0-or-later
// Source pins and numerical deviations: THIRD_PARTY.md.
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

// Packed RGB box mean, shared by camera/color nodes and all compute backends.
// One invocation owns one output channel. Two passes provide the same finite
// headroom as Bayer reduction without requiring device f64 support.
#[cube(launch)]
pub fn reduce_rgb(
    input: &[f32],
    output: &mut [f32],
    width: usize,
    height: usize,
    out_width: usize,
    factor: usize,
) {
    let i = ABSOLUTE_POS;
    if i < output.len() {
        let pixel = i / 3;
        let channel = i % 3;
        let x = pixel % out_width * factor;
        let y = pixel / out_width * factor;
        let end_x = (x + factor).min(width);
        let end_y = (y + factor).min(height);
        let mut magnitude = 0.0f32;
        for row in y..end_y {
            for col in x..end_x {
                magnitude = magnitude.max(input[(row * width + col) * 3 + channel].abs());
            }
        }
        let rescale = magnitude > 1.844_674_4e19_f32 && magnitude <= f32::MAX;
        let mut sum = 0.0f32;
        for row in y..end_y {
            for col in x..end_x {
                let value = input[(row * width + col) * 3 + channel];
                // Preserve NaN/Inf propagation when another sample needs scaling.
                sum += if rescale && value.abs() <= f32::MAX { down64(value) } else { value };
            }
        }
        let mean = sum / ((end_x - x) * (end_y - y)) as f32;
        output[i] = if rescale && mean.abs() <= f32::MAX {
            let bound = down64(magnitude);
            mean.clamp(-bound, bound) * 1.844_674_4e19_f32
        } else {
            mean
        };
    }
}
