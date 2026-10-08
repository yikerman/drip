// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2022-2026 darktable developers.
//! Opposed-channel reconstruction with GPU-resident mask and reduction passes.
//! Fixed row order and compensated f32 accumulation replace the CPU reference's
//! f64 sums because portable WGSL has no f64. See the shader and THIRD_PARTY.md.

use crate::compute::{Compute, GpuBuffer};
use crate::image::{Gpu, Mosaic};
use crate::node::KernelError;

pub(super) fn process(
    compute: &Compute,
    m: &Mosaic<Gpu>,
    threshold: f32,
) -> Result<GpuBuffer, KernelError> {
    super::super::gpu::require_bayer(m)?;
    if !threshold.is_finite() || threshold <= 0.0 {
        return Err(KernelError::Failed("finite positive clipping threshold required".into()));
    }
    let n = m.width() * m.height();
    let cells = m.width().div_ceil(3) * m.height().div_ceil(3);
    let output = compute.allocate_f32(n)?;
    let mask = compute.allocate_f32(cells * 3)?;
    let near = compute.allocate_f32(cells * 3)?;
    let rows = compute.allocate_f32(m.height() * 6)?;
    let chroma = compute.allocate_f32(3)?;
    let mut parameters = vec![m.width() as f32, m.height() as f32];
    parameters.extend(m.interpretation().cfa.colors.iter().map(|&c| c as f32));
    parameters.extend(m.interpretation().white.map(|v| v * threshold));
    parameters.push(0.0);
    for (stage, invocations) in [cells, cells, m.height(), 1, n].into_iter().enumerate() {
        parameters[10] = stage as f32;
        let params = compute.upload_f32(&parameters)?;
        compute.dispatch(
            "opposed",
            include_str!("../shaders/opposed.wgsl"),
            "opposed",
            &[m.gpu_buffer(), &output, &mask, &near, &rows, &chroma, &params],
            super::super::gpu::groups(compute, invocations),
        )?;
    }
    Ok(output)
}

#[cfg(test)]
#[path = "opposed_reference.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Camera, Cfa, RawMat, SensorMosaic};
    use std::sync::Arc;

    #[test]
    fn gpu_matches_opposed_reference_and_preserves_unclipped_samples() {
        let compute = crate::nodes::gpu::test_compute();
        for (phase, colors) in
            [[0, 1, 3, 2], [1, 0, 2, 3], [3, 2, 0, 1], [2, 3, 1, 0]].into_iter().enumerate()
        {
            for (width, height) in [(2, 2), (5, 7), (96, 90), (97, 91)] {
                let cfa = Cfa { size: 2, colors: colors.to_vec() };
                let white = [1.3, 1.0, 1.7, 1.0];
                let samples = (0..width * height)
                    .map(|i| {
                        let (r, c) = (i / width, i % width);
                        let ch = cfa.color(r, c) as usize;
                        let value = if (32..58).contains(&r) && (32..64).contains(&c) {
                            if ch == 0 { 1.0 } else { 0.9 }
                        } else {
                            0.35 + ((r * 13 + c * 7) % 31) as f32 / 100.0
                        };
                        value * white[ch]
                    })
                    .collect();
                let input = Mosaic::new(
                    Arc::new(RawMat::from_samples(width, height, 1, samples)),
                    SensorMosaic {
                        cfa,
                        white,
                        camera: Arc::new(Camera {
                            xyz_to_cam: [[0.0; 3]; 3],
                            white_balance: white,
                        }),
                    },
                );
                let expected = reference::process(&input, 0.98);
                let gpu = input.upload(compute).unwrap();
                let result = process(compute, &gpu, 0.98).unwrap();
                let actual = compute.read_f32(&result).unwrap();
                if (width, height) == (96, 90) {
                    for line in include_str!("../../../tests/reference/opposed.csv")
                        .lines()
                        .filter(|line| !line.starts_with('#'))
                    {
                        let values: Vec<f32> =
                            line.split(',').map(|s| s.parse().unwrap()).collect();
                        if values[0] as usize != phase {
                            continue;
                        }
                        let pixel = values[1] as usize * width + values[2] as usize;
                        assert!((actual[pixel] - values[3]).abs() < 2e-5);
                    }
                }
                for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
                    assert!(
                        (a - e).abs() < 2e-5,
                        "{width}x{height}, {colors:?}, sample {i}: {a} != {e}"
                    );
                    let ch = input.interpretation().cfa.color(i / width, i % width) as usize;
                    if input.samples()[i] < white[ch] * 0.98 {
                        assert_eq!(a, input.samples()[i]);
                    } else {
                        assert!(a >= input.samples()[i]);
                    }
                }
            }
        }
    }
}
