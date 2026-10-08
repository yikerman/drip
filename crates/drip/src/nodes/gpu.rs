//! Shared dispatch mechanics; semantic checks remain beside their node.

use crate::compute::{Compute, GpuBuffer};
use crate::image::{Gpu, Mosaic};
use crate::node::KernelError;

pub(super) const POINTWISE: &str = include_str!("shaders/pointwise.wgsl");

/// Cover large photographs without exceeding the per-axis dispatch limit.
/// Every kernel flattens the two-dimensional grid in the same order.
pub(super) fn groups(compute: &Compute, invocations: usize) -> [u32; 3] {
    let count = invocations.div_ceil(256) as u32;
    let width = count.min(compute.device().limits().max_compute_workgroups_per_dimension).max(1);
    [width, count.div_ceil(width), 1]
}

pub(super) fn pointwise(
    compute: &Compute,
    entry: &'static str,
    source: &GpuBuffer,
    output_len: usize,
    invocations: usize,
    parameters: &[f32],
) -> Result<GpuBuffer, KernelError> {
    let output = compute.allocate_f32(output_len)?;
    let parameters = compute.upload_f32(parameters)?;
    compute.dispatch(
        entry,
        POINTWISE,
        entry,
        &[source, &output, &parameters],
        groups(compute, invocations),
    )?;
    Ok(output)
}

/// RCD, binning and opposed reconstruction assume a 2×2 Bayer cell with
/// diagonally opposite red/blue sites and the two intervening green sites.
pub(super) fn require_bayer(m: &Mosaic<Gpu>) -> Result<(), KernelError> {
    let cfa = &m.interpretation().cfa;
    let colors: Vec<_> = cfa.colors.iter().map(|&c| if c == 3 { 1 } else { c }).collect();
    if cfa.size != 2
        || !matches!(colors.as_slice(), [0, 1, 1, 2] | [1, 0, 2, 1] | [1, 2, 0, 1] | [2, 1, 1, 0])
    {
        return Err(KernelError::Failed("a 2 × 2 Bayer pattern is required".into()));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn test_compute() -> &'static Compute {
    static COMPUTE: std::sync::OnceLock<std::sync::Arc<Compute>> = std::sync::OnceLock::new();
    COMPUTE.get_or_init(|| {
        Compute::new().expect("GPU tests require a Vulkan/Metal/DX12 adapter or software Vulkan")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointwise_kernels_match_independent_formulas_and_preserve_bayer_phase() {
        let compute = test_compute();
        let input: Vec<_> = (0..4099).map(|i| i as f32 / 71.0 - 2.0).collect();
        let source = compute.upload_f32(&input).unwrap();
        let result =
            pointwise(compute, "exposure", &source, input.len(), input.len(), &[2.0]).unwrap();
        assert_eq!(
            compute.read_f32(&result).unwrap(),
            input.iter().map(|v| v * 2.0).collect::<Vec<_>>()
        );

        let pixels: Vec<_> = (0..4097)
            .map(|i| {
                std::array::from_fn::<_, 3, _>(|c| ((i * 3 + c) * 37 % 1009) as f32 / 71.0 - 2.0)
            })
            .collect();
        let matrix = [[1.2, -0.1, -0.1], [-0.3, 1.5, -0.2], [0.0, -0.1, 1.1]];
        let source = compute.upload_f32(pixels.as_flattened()).unwrap();
        let result = pointwise(
            compute,
            "matrix",
            &source,
            pixels.len() * 3,
            pixels.len(),
            matrix.as_flattened(),
        )
        .unwrap();
        for (actual, pixel) in
            compute.read_f32(&result).unwrap().as_chunks::<3>().0.iter().zip(&pixels)
        {
            for (&a, row) in actual.iter().zip(matrix) {
                let expected: f64 =
                    row.iter().zip(pixel).map(|(&a, &b)| f64::from(a) * f64::from(b)).sum();
                assert!((f64::from(a) - expected).abs() < 4e-6);
            }
        }
        let (width, height) = (19, 17);
        let input: Vec<_> = (0..width * height).map(|i| i as f32 - 100.0).collect();
        let source = compute.upload_f32(&input).unwrap();
        for colors in [[0, 1, 1, 2], [1, 0, 2, 1], [1, 2, 0, 1], [2, 1, 1, 0]] {
            let gains = [2.0, 1.0, 3.0, 4.0];
            let balanced = pointwise(
                compute,
                "white_balance",
                &source,
                input.len(),
                input.len(),
                &[width as f32, 2.0, gains[0], gains[1], gains[2], gains[3]],
            )
            .unwrap();
            let host = compute.read_f32(&balanced).unwrap();
            for (i, &actual) in host.iter().enumerate() {
                assert_eq!(actual, input[i] * gains[i / width % 2 * 2 + i % width % 2]);
            }
            let count = (width / 2) * (height / 2);
            let mut parameters = vec![width as f32, height as f32, 2.0];
            parameters.extend(colors.map(|c| c as f32));
            let result =
                pointwise(compute, "bin2x2", &balanced, count * 3, count, &parameters).unwrap();
            for (i, actual) in
                compute.read_f32(&result).unwrap().as_chunks::<3>().0.iter().enumerate()
            {
                let origin = i / (width / 2) * 2 * width + i % (width / 2) * 2;
                let mut expected = [0.0; 3];
                for (phase, offset) in [0, 1, width, width + 1].into_iter().enumerate() {
                    expected[colors[phase]] += host[origin + offset];
                }
                expected[1] /= 2.0;
                assert_eq!(*actual, expected);
            }
        }
        let count = (width / 4 * 2) * (height / 4 * 2);
        let result =
            pointwise(compute, "downsample", &source, count, count, &[width as f32, height as f32])
                .unwrap();
        for (i, &actual) in compute.read_f32(&result).unwrap().iter().enumerate() {
            let row = i / (width / 4 * 2);
            let col = i % (width / 4 * 2);
            let j = (row / 2 * 4 + row % 2) * width + col / 2 * 4 + col % 2;
            assert_eq!(
                actual,
                (input[j] + input[j + 2] + input[j + 2 * width] + input[j + 2 * width + 2]) * 0.25
            );
        }
    }
}
