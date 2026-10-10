//! Device reductions. Only compact integer counts cross back to the GUI.
use crate::node_ui::data::{DeviceRgb, DisplayInterpretation, PrepareContext};
use cubecl::{bytes::Bytes, prelude::*, server::Handle};
use drip::{Error, Result};

const BINS: usize = 256;
const SHARDS: usize = 32;

fn argument(handle: &Handle) -> BufferArg {
    // SAFETY: all buffers here are scalar f32/u32 arrays, matching kernel slices.
    unsafe { BufferArg::from_raw_parts(handle.clone(), 1) }
}

fn zeroed(client: &Client, len: usize) -> Handle {
    let handle = client.empty(len * 4);
    kernels::clear::launch(
        client,
        CubeCount::Static(len.div_ceil(256) as u32, 1, 1),
        CubeDim::new_1d(256),
        argument(&handle),
    );
    handle
}

fn read(client: &Client, handle: Handle) -> Result<Vec<u32>> {
    let bytes = client.read_one(handle).map_err(|e| Error::Runtime(e.to_string()))?;
    Ok(bytes.as_chunks::<4>().0.iter().map(|&v| u32::from_ne_bytes(v)).collect())
}

pub fn exposure<I: DisplayInterpretation>(
    image: &DeviceRgb<I>,
    ctx: &PrepareContext,
    min: f32,
    max: f32,
    waveform: bool,
) -> Result<Vec<[u32; 3]>> {
    let client = ctx.client()?;
    // Barrier kernels need one runnable worker per lane on the CPU runtime.
    // Respect its reported workgroup limit instead of forcing 256 host threads.
    let dim = CubeDim::new_1d(client.properties().hardware.max_units_per_cube.min(256));
    // Upload the same f32 edges as the host implementation: comparing edges
    // preserves exact exposure boundaries without device log2 rounding.
    let edges: Vec<f32> =
        (0..BINS).map(|i| 2f32.powf(min + i as f32 * (max - min) / BINS as f32)).collect();
    let edges = client.create(Bytes::from_elems(edges));
    let output = if waveform {
        let output = zeroed(client, BINS * BINS * 3);
        kernels::exposure_counts::launch(
            client,
            CubeCount::Static(256, image.desc.extent.height.div_ceil(32).min(256), 1),
            dim,
            image.pixels.argument(),
            argument(&edges),
            argument(&output),
            image.desc.extent.width as usize,
            true,
        );
        output
    } else {
        let groups = image.desc.extent.pixels().div_ceil(4096).clamp(1, 256);
        let partial = client.empty(groups * BINS * 3 * 4);
        kernels::exposure_counts::launch(
            client,
            CubeCount::Static(groups as u32, 1, 1),
            dim,
            image.pixels.argument(),
            argument(&edges),
            argument(&partial),
            image.desc.extent.width as usize,
            false,
        );
        let output = client.empty(BINS * 3 * 4);
        kernels::merge::launch(
            client,
            CubeCount::Static(3, 1, 1),
            CubeDim::new_1d(256),
            argument(&partial),
            argument(&output),
            groups,
        );
        output
    };
    Ok(read(client, output)?.as_chunks::<3>().0.to_vec())
}

pub fn vectorscope<I: DisplayInterpretation>(
    image: &DeviceRgb<I>,
    ctx: &PrepareContext,
    matrix: &drip::node::color::Mat3,
    white: [f32; 2],
) -> Result<Vec<[u32; 3]>> {
    let client = ctx.client()?;
    let coefficients = client.create(Bytes::from_elems(
        matrix.iter().flatten().map(|&v| v as f32).chain(white).collect::<Vec<_>>(),
    ));
    // Separate warp lanes into shards so neutral pixels do not serialize all
    // increments at a single address. Scratch is bounded independently of image size.
    let partial = zeroed(client, SHARDS * BINS * BINS);
    kernels::vector_counts::launch(
        client,
        CubeCount::Static(256, 1, 1),
        CubeDim::new_1d(256),
        image.pixels.argument(),
        argument(&coefficients),
        argument(&partial),
    );
    let output = client.empty(BINS * BINS * 4);
    kernels::merge::launch(
        client,
        CubeCount::Static(256, 1, 1),
        CubeDim::new_1d(256),
        argument(&partial),
        argument(&output),
        SHARDS,
    );
    Ok(read(client, output)?.into_iter().map(|c| [c, 0, 0]).collect())
}

mod kernels {
    use cubecl::prelude::*;

    #[cube(launch)]
    pub fn clear(output: &mut [u32]) {
        if ABSOLUTE_POS < output.len() {
            output[ABSOLUTE_POS] = 0;
        }
    }

    #[cube]
    fn exposure_bin(value: f32, edges: &[f32]) -> usize {
        let mut lo = 0usize;
        let mut hi = 256usize;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if value >= edges[mid] {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        usize::max(lo, 1) - 1
    }

    #[cube(launch)]
    pub fn exposure_counts(
        input: &[f32],
        edges: &[f32],
        output: &mut [Atomic<u32>],
        width: usize,
        #[comptime] waveform: bool,
    ) {
        let counts = Shared::<[Atomic<u32>]>::new_slice(768usize);
        let mut bin = UNIT_POS as usize;
        while bin < 256 {
            for c in 0..3usize {
                counts[bin * 3 + c].store(0);
            }
            bin += CUBE_DIM as usize;
        }
        sync_cube();
        if waveform {
            let column = CUBE_POS_X as usize;
            let start = column * (width / 256) + (column * (width % 256)).div_ceil(256);
            let end = (column + 1) * (width / 256) + ((column + 1) * (width % 256)).div_ceil(256);
            let columns = end - start;
            if columns > 0 {
                let height = input.len() / 3 / width;
                let rows_per_group = height.div_ceil(CUBE_COUNT_Y as usize);
                let row = usize::min(CUBE_POS_Y as usize * rows_per_group, height);
                let rows = usize::min(rows_per_group, height - row);
                let mut i = UNIT_POS as usize;
                while i < columns * rows {
                    let pixel = (row + i / columns) * width + start + i % columns;
                    for c in 0..3usize {
                        let bin = exposure_bin(input[pixel * 3 + c], edges);
                        counts[bin * 3 + c].fetch_add(1);
                    }
                    i += CUBE_DIM as usize;
                }
            }
        } else {
            let mut pixel = ABSOLUTE_POS;
            while pixel < input.len() / 3 {
                for c in 0..3usize {
                    let bin = exposure_bin(input[pixel * 3 + c], edges);
                    counts[bin * 3 + c].fetch_add(1);
                }
                pixel += CUBE_COUNT * (CUBE_DIM as usize);
            }
        }
        sync_cube();
        let mut bin = UNIT_POS as usize;
        while bin < 256 {
            for c in 0..3usize {
                let value = counts[bin * 3 + c].load();
                if waveform {
                    if value > 0 {
                        output[((255 - bin) * 256 + CUBE_POS_X as usize) * 3 + c].fetch_add(value);
                    }
                } else {
                    output[CUBE_POS * 768 + bin * 3 + c].store(value);
                }
            }
            bin += CUBE_DIM as usize;
        }
    }

    #[cube(launch)]
    pub fn merge(partial: &[u32], output: &mut [u32], shards: usize) {
        let i = ABSOLUTE_POS;
        if i < output.len() {
            let mut sum = 0u32;
            for shard in 0..shards {
                sum += partial[shard * output.len() + i];
            }
            output[i] = sum;
        }
    }

    #[cube]
    fn positive(v: f32) -> f32 {
        if v > 0.0f32 { v } else { 0.0f32 }
    }

    #[cube(launch)]
    pub fn vector_counts(input: &[f32], coefficients: &[f32], output: &mut [Atomic<u32>]) {
        let mut pixel = ABSOLUTE_POS;
        let shard = (UNIT_POS as usize) % 32 * 65536;
        let mut previous = 65536usize;
        let mut count = 0u32;
        while pixel < input.len() / 3 {
            let r = positive(input[pixel * 3]);
            let g = positive(input[pixel * 3 + 1]);
            let b = positive(input[pixel * 3 + 2]);
            // Normalize first: chromaticity is exposure independent and finite
            // HDR samples must not overflow XYZ or its denominator.
            let peak = f32::max(r, f32::max(g, b));
            if peak > 0.0 {
                let r = r / peak;
                let g = g / peak;
                let b = b / peak;
                let x = coefficients[0] * r + coefficients[1] * g + coefficients[2] * b;
                let y = coefficients[3] * r + coefficients[4] * g + coefficients[5] * b;
                let z = coefficients[6] * r + coefficients[7] * g + coefficients[8] * b;
                let denominator = x + 15.0 * y + 3.0 * z;
                let u = 4.0 * x / denominator;
                let v = 9.0 * y / denominator;
                let bx =
                    usize::cast_from(f32::clamp((0.5 + u - coefficients[9]) * 256.0, 0.0, 255.0));
                let by =
                    usize::cast_from(f32::clamp((0.5 - v + coefficients[10]) * 256.0, 0.0, 255.0));
                let mut bin = by * 256 + bx;
                // Exact neutrals belong at D65, including values at f32 extremes.
                if r == g && g == b {
                    bin = 128usize * 256 + 128;
                }
                // As in the host reference, undefined infinite chromaticities
                // enter the top-left bin; NaN and negative channels clip to zero.
                if peak > f32::MAX {
                    bin = 0usize;
                }
                if bin != previous && count > 0 {
                    output[shard + previous].fetch_add(count);
                    count = 0;
                }
                previous = bin;
                count += 1;
            }
            pixel += CUBE_COUNT * (CUBE_DIM as usize);
        }
        if count > 0 {
            output[shard + previous].fetch_add(count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_counts_cover_all_bins_with_small_workgroups() {
        let ctx = crate::node_ui::scopes::tests::context();
        let client = ctx.client().unwrap();
        let edges: Vec<f32> = (0..256).map(|i| 2f32.powf(-12.0 + i as f32 / 16.0)).collect();
        let input = client
            .create(Bytes::from_elems(edges.iter().flat_map(|&v| [v; 3]).collect::<Vec<_>>()));
        let edges = client.create(Bytes::from_elems(edges));
        let dim = CubeDim::new_1d(client.properties().hardware.max_units_per_cube.min(3));
        for waveform in [false, true] {
            let len = if waveform { 256 * 256 * 3 } else { 256 * 3 };
            let output = zeroed(client, len);
            kernels::exposure_counts::launch(
                client,
                CubeCount::Static(if waveform { 256 } else { 1 }, 1, 1),
                dim,
                argument(&input),
                argument(&edges),
                argument(&output),
                256,
                waveform,
            );
            let counts = read(client, output).unwrap();
            assert_eq!(counts.iter().sum::<u32>(), 256 * 3);
            for bin in 0..256 {
                let index = if waveform { ((255 - bin) * 256 + bin) * 3 } else { bin * 3 };
                assert_eq!(&counts[index..index + 3], &[1; 3]);
            }
        }
    }
}
