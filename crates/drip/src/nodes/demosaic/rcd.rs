// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2010-2026 darktable developers.
//! RCD's ratio estimates and directional color differences in four GPU passes.
//! The ten-pixel bilinear border preserves measured sensor samples. Full-image
//! scratch replaces CPU tile halos; it changes scheduling, not the estimator.
//! See `shaders/rcd.wgsl` and THIRD_PARTY.md for upstream authors and revision.

use crate::compute::{Compute, GpuBuffer};
use crate::image::{Gpu, Mosaic};
use crate::node::KernelError;

pub(super) fn process(compute: &Compute, m: &Mosaic<Gpu>) -> Result<GpuBuffer, KernelError> {
    super::super::gpu::require_bayer(m)?;
    let n = m.width() * m.height();
    let a = compute.allocate_f32(n * 3)?;
    let b = compute.allocate_f32(n * 3)?;
    let maps = compute.allocate_f32(n * 3)?;
    let colors = m.interpretation().cfa.colors.iter().map(|&c| if c == 3 { 1.0 } else { c as f32 });
    let mut parameters = vec![m.width() as f32, m.height() as f32];
    parameters.extend(colors);
    parameters.push(0.0);
    for stage in 0..4 {
        parameters[6] = stage as f32;
        let params = compute.upload_f32(&parameters)?;
        let (previous, target) = if stage % 2 == 0 { (&b, &a) } else { (&a, &b) };
        compute.dispatch(
            "rcd",
            include_str!("../shaders/rcd.wgsl"),
            "rcd",
            &[m.gpu_buffer(), previous, target, &maps, &params],
            super::super::gpu::groups(compute, n),
        )?;
    }
    Ok(b)
}

#[cfg(test)]
#[path = "rcd_reference.rs"]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Camera, Cfa, RawMat, SensorMosaic};
    use std::sync::Arc;

    #[test]
    fn gpu_matches_rcd_reference_across_phases_edges_and_old_tile_seams() {
        let compute = crate::nodes::gpu::test_compute();
        for (phase, colors) in
            [[0, 1, 3, 2], [1, 0, 2, 3], [3, 2, 0, 1], [2, 3, 1, 0]].into_iter().enumerate()
        {
            for (width, height) in [(2, 2), (19, 17), (277, 269)] {
                let input = Mosaic::new(
                    Arc::new(RawMat::from_samples(
                        width,
                        height,
                        1,
                        (0..width * height)
                            .map(|i| ((i * 137 + i / width * 29) % 2048) as f32 / 1024.0 - 0.03)
                            .collect(),
                    )),
                    SensorMosaic {
                        cfa: Cfa { size: 2, colors: colors.to_vec() },
                        white: [1.0; 4],
                        camera: Arc::new(Camera {
                            xyz_to_cam: [[0.0; 3]; 3],
                            white_balance: [1.0; 4],
                        }),
                    },
                );
                let expected = reference::process(&input);
                let gpu = input.upload(compute).unwrap();
                let result = process(compute, &gpu).unwrap();
                let actual = compute.read_f32(&result).unwrap();
                if (width, height) == (277, 269) {
                    for line in include_str!("../../../tests/reference/rcd.csv")
                        .lines()
                        .filter(|line| !line.starts_with('#'))
                    {
                        let values: Vec<f32> =
                            line.split(',').map(|s| s.parse().unwrap()).collect();
                        if values[0] as usize != phase {
                            continue;
                        }
                        let pixel = values[1] as usize * width + values[2] as usize;
                        for ch in 0..3 {
                            assert!((actual[pixel * 3 + ch] - values[3 + ch]).abs() < 2e-5);
                        }
                    }
                }
                for (i, (&a, &e)) in actual.iter().zip(expected.as_flattened()).enumerate() {
                    assert!(
                        (a - e).abs() < 2e-5,
                        "{width}x{height}, {colors:?}, sample {i}: {a} != {e}"
                    );
                }
            }
        }
    }
}
