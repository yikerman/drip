// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2022-2026 darktable developers.
//! Inpaint opposed, developed by garagecoder and Iain of G'MIC and Hanno
//! Schwalm of darktable \[1\]. Estimate a clipped channel from the cube-root
//! average of the other two; learn a chrominance offset near clipped areas.
//! Drip processes as-shot-balanced Bayer data (no late chromatic adaptation).
//! Partial mask cells and the last sensor row/column are included; dilation
//! is clipped at image edges. CubeCL uses f32 cube roots and fixed fan-in
//! reductions instead of upstream's CPU accumulation; counts remain exact u32.
//!
//! \[1\] darktable developers, “opposed.c” and “segbased.c,” commit
//! 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/hlreconstruct>
//! See THIRD_PARTY.md for credits and license.

use crate::{
    Result,
    node::data::{Bayer, DeviceBuffer},
    ports::{Device, Read},
    runtime::{KernelContext, dispatch_dims},
};
use cubecl::{prelude::*, server::Handle};

// Masks are three channel bits; counts are integers even above 2^24 samples.
struct U32Buffer(Handle);
impl U32Buffer {
    fn allocate(ctx: &KernelContext<'_>, elements: usize) -> Result<Self> {
        Ok(Self(ctx.client()?.empty(elements * 4)))
    }
    fn argument(&self) -> BufferArg {
        // SAFETY: this private scratch holds contiguous scalar u32 words.
        unsafe { BufferArg::from_raw_parts(self.0.clone(), 1) }
    }
}
struct Partial {
    sums: DeviceBuffer<()>,
    counts: U32Buffer,
}
impl Partial {
    fn allocate(ctx: &KernelContext<'_>, groups: usize) -> Result<Self> {
        Ok(Self {
            sums: ctx.scratch_f32(groups * 3)?,
            counts: U32Buffer::allocate(ctx, groups * 3)?,
        })
    }
}

pub(super) fn process(
    ctx: &KernelContext<'_>,
    image: &Read<'_, Device<Bayer>>,
    clips: [f32; 4],
    output: &mut DeviceBuffer<Bayer>,
) -> Result<()> {
    let (w, h) = (image.desc.extent.width as usize, image.desc.extent.height as usize);
    let phase = image.desc.phase as usize;
    let (mw, mh) = (w.div_ceil(3), h.div_ceil(3));
    let mask = U32Buffer::allocate(ctx, mw * mh)?;
    let near = U32Buffer::allocate(ctx, mw * mh)?;
    let client = ctx.client()?;
    let (count, dim) = dispatch_dims(mw * mh);
    clipped_cells::launch(
        client,
        count.clone(),
        dim,
        image.data.argument(),
        mask.argument(),
        w,
        h,
        mw,
        clips[0],
        clips[1],
        clips[2],
        clips[3],
        phase,
    );
    dilate::launch(client, count, dim, mask.argument(), near.argument(), mw, mh);

    // Each invocation owns one fixed 256-sample block. Subsequent passes reduce
    // 256 partials at a time, with no atomics or cross-workgroup communication.
    let mut groups = image.desc.extent.pixels().div_ceil(256);
    let mut partial = Partial::allocate(ctx, groups)?;
    let (count, dim) = dispatch_dims(groups);
    chrominance::launch(
        client,
        count,
        dim,
        image.data.argument(),
        near.argument(),
        partial.sums.argument(),
        partial.counts.argument(),
        w,
        h,
        mw,
        clips[0],
        clips[1],
        clips[2],
        clips[3],
        phase,
    );
    while groups > 1 {
        let next_groups = groups.div_ceil(256);
        let next = Partial::allocate(ctx, next_groups)?;
        let (count, dim) = dispatch_dims(next_groups);
        reduce_chrominance::launch(
            client,
            count,
            dim,
            partial.sums.argument(),
            partial.counts.argument(),
            next.sums.argument(),
            next.counts.argument(),
            groups,
        );
        partial = next;
        groups = next_groups;
    }
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    reconstruct::launch(
        client,
        count,
        dim,
        image.data.argument(),
        partial.sums.argument(),
        partial.counts.argument(),
        output.argument(),
        w,
        h,
        clips[0],
        clips[1],
        clips[2],
        clips[3],
        phase,
    );
    Ok(())
}

#[cube]
fn site(row: usize, col: usize) -> usize {
    row % 2 * 2 + col % 2
}
#[cube]
fn clip(position: usize, c0: f32, c1: f32, c2: f32, c3: f32) -> f32 {
    if position == 0 {
        c0
    } else if position == 1 {
        c1
    } else if position == 2 {
        c2
    } else {
        c3
    }
}
#[cube]
fn reference(input: &[f32], row: usize, col: usize, w: usize, h: usize, phase: usize) -> f32 {
    let mut sums = Array::<f32>::new(3usize);
    let mut counts = Array::<f32>::new(3usize);
    for ch in 0..3 {
        sums[ch] = 0.0f32;
        counts[ch] = 0.0f32;
    }
    for dy in 0..3 {
        for dx in 0..3 {
            let r = row as i32 + dy - 1;
            let c = col as i32 + dx - 1;
            if r >= 0 && c >= 0 && r < h as i32 && c < w as i32 {
                let ch = crate::node::shared_kernel::color(r as usize, c as usize, phase);
                sums[ch] += input[r as usize * w + c as usize].max(0.0f32);
                counts[ch] += 1.0f32;
            }
        }
    }
    let ch = crate::node::shared_kernel::color(row, col, phase);
    let a = (ch + 1) % 3;
    let b = (ch + 2) % 3;
    let opposite = 0.5f32
        * ((sums[a] / counts[a].max(1.0f32)).powf(1.0f32 / 3.0f32)
            + (sums[b] / counts[b].max(1.0f32)).powf(1.0f32 / 3.0f32));
    opposite * opposite * opposite
}

#[cube(launch)]
fn clipped_cells(
    input: &[f32],
    mask: &mut [u32],
    w: usize,
    h: usize,
    mw: usize,
    c0: f32,
    c1: f32,
    c2: f32,
    c3: f32,
    phase: usize,
) {
    let i = ABSOLUTE_POS;
    if i < mask.len() {
        let row = i / mw * 3;
        let col = i % mw * 3;
        let mut bits = 0u32;
        for dy in 0..3 {
            for dx in 0..3 {
                let r = row + dy;
                let c = col + dx;
                if r < h && c < w && input[r * w + c] >= clip(site(r, c), c0, c1, c2, c3) {
                    bits |= 1u32 << crate::node::shared_kernel::color(r, c, phase) as u32;
                }
            }
        }
        mask[i] = bits;
    }
}
#[cube(launch)]
fn dilate(mask: &[u32], near: &mut [u32], mw: usize, mh: usize) {
    let i = ABSOLUTE_POS;
    if i < near.len() {
        let row = i / mw;
        let col = i % mw;
        let mut bits = 0u32;
        for dy in 0..7 {
            for dx in 0..7 {
                let r = row as i32 + dy - 3;
                let c = col as i32 + dx - 3;
                // Upstream's 7x7 footprint omits its four corners.
                let corner = (dy == 0 || dy == 6) && (dx == 0 || dx == 6);
                if !corner && r >= 0 && c >= 0 && r < mh as i32 && c < mw as i32 {
                    bits |= mask[r as usize * mw + c as usize];
                }
            }
        }
        near[i] = bits;
    }
}
#[cube(launch)]
fn chrominance(
    input: &[f32],
    near: &[u32],
    sums: &mut [f32],
    counts: &mut [u32],
    w: usize,
    h: usize,
    mw: usize,
    c0: f32,
    c1: f32,
    c2: f32,
    c3: f32,
    phase: usize,
) {
    let group = ABSOLUTE_POS;
    if group * 3 < sums.len() {
        let mut sum = Array::<f32>::new(3usize);
        let mut count = Array::<u32>::new(3usize);
        for ch in 0..3 {
            sum[ch] = 0.0f32;
            count[ch] = 0u32;
        }
        for offset in 0..256 {
            let i = group * 256 + offset;
            if i < input.len() {
                let r = i / w;
                let c = i % w;
                let ch = crate::node::shared_kernel::color(r, c, phase);
                let level = clip(site(r, c), c0, c1, c2, c3);
                let value = input[i];
                if value > 0.2f32 * level
                    && value < level
                    && (near[r / 3 * mw + c / 3] & (1u32 << ch as u32)) != 0
                {
                    sum[ch] += value - reference(input, r, c, w, h, phase);
                    count[ch] += 1u32;
                }
            }
        }
        for ch in 0..3 {
            sums[group * 3 + ch] = sum[ch];
            counts[group * 3 + ch] = count[ch];
        }
    }
}
#[cube(launch)]
fn reduce_chrominance(
    sums: &[f32],
    counts: &[u32],
    output_sums: &mut [f32],
    output_counts: &mut [u32],
    groups: usize,
) {
    let group = ABSOLUTE_POS;
    if group * 3 < output_sums.len() {
        for ch in 0..3 {
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for offset in 0..256 {
                let i = group * 256 + offset;
                if i < groups {
                    sum += sums[i * 3 + ch];
                    count += counts[i * 3 + ch];
                }
            }
            output_sums[group * 3 + ch] = sum;
            output_counts[group * 3 + ch] = count;
        }
    }
}
#[cube(launch)]
fn reconstruct(
    input: &[f32],
    sums: &[f32],
    counts: &[u32],
    output: &mut [f32],
    w: usize,
    h: usize,
    c0: f32,
    c1: f32,
    c2: f32,
    c3: f32,
    phase: usize,
) {
    let i = ABSOLUTE_POS;
    if i < input.len() {
        let r = i / w;
        let c = i % w;
        let value = input[i];
        let mut result = value;
        if value >= clip(site(r, c), c0, c1, c2, c3) {
            let ch = crate::node::shared_kernel::color(r, c, phase);
            let mut chroma = 0.0f32;
            if counts[ch] > 100u32 {
                chroma = sums[ch] / counts[ch] as f32;
            }
            result = value.max(reference(input, r, c, w, h, phase) + chroma);
        }
        output[i] = result;
    }
}
