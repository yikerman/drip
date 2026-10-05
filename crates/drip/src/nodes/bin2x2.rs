//! Half-size Bayer cell averaging.

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::Params;
use crate::value::{PortType, Rgb, Value};
use rayon::prelude::*;
use std::sync::Arc;

/// Naive debayering: each 2 × 2 Bayer cell becomes one pixel, averaging its
/// two greens. Halves the resolution (DESIGN C6).
pub static BIN_2X2: NodeKind = NodeKind {
    name: "demosaic.bin2x2",
    label: "debayer",
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: &[PortType::Mosaic] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::CameraRgb }],
    eval: bin_2x2,
    actions: &[],
};

fn bin_2x2(_: Params, inputs: &[Value], ctx: &EvalContext) -> Result<Evaluated, String> {
    let m = super::demosaic::preview(inputs[0].mosaic(), ctx);
    let (width, height) = (m.width / 2, m.height / 2);
    // The second green (3) joins the first.
    let colors: Vec<u32> = m.cfa.colors.iter().map(|&c| [0, 1, 2, 1][c as usize]).collect();
    let pixels = debayer(&m.data, m.width, m.height, m.cfa.size, &colors);
    super::single(Value::CameraRgb(
        Arc::new(Rgb { width, height, scale: m.scale * 2, pixels }),
        m.camera.clone(),
    ))
}

pub fn debayer(
    input: &[f32],
    width: usize,
    height: usize,
    cfa_size: usize,
    colors: &[u32],
) -> Vec<[f32; 3]> {
    (0..(width / 2) * (height / 2))
        .into_par_iter()
        .map(|i| {
            let row = i / (width / 2) * 2;
            let col = i % (width / 2) * 2;
            let (mut sum, mut count) = ([0.0; 3], [0.0; 3]);
            for r in 0..2 {
                for c in 0..2 {
                    let phase = ((row + r) % cfa_size) * cfa_size + (col + c) % cfa_size;
                    let color = colors[phase] as usize;
                    sum[color] += input[(row + r) * width + col + c];
                    count[color] += 1.0;
                }
            }
            std::array::from_fn(|c| sum[c] / count[c])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::white_balance::process as white_balance;
    use super::*;

    #[test]
    fn cfa_kernels_cover_phases_and_crop_odd_edges() {
        let (width, height) = (19, 17);
        for colors in [[0, 1, 1, 2], [1, 0, 2, 1], [1, 2, 0, 1], [2, 1, 1, 0]] {
            let gains = [2.0, 1.0, 3.0, 4.0];
            let input: Vec<_> = (0..width * height).map(|i| i as f32 - 100.0).collect();
            let balanced = white_balance(&input, width, 2, &gains);
            for (i, &value) in balanced.iter().enumerate() {
                assert_eq!(value, input[i] * gains[i / width % 2 * 2 + i % width % 2]);
            }
            let pixels = debayer(&balanced, width, height, 2, &colors);
            for (i, pixel) in pixels.iter().enumerate() {
                let origin = i / (width / 2) * 2 * width + i % (width / 2) * 2;
                let mut expected = [0.0; 3];
                for (phase, offset) in [0, 1, width, width + 1].into_iter().enumerate() {
                    expected[colors[phase] as usize] += balanced[origin + offset];
                }
                expected[1] /= 2.0;
                assert_eq!(*pixel, expected);
            }
        }
    }
    #[test]
    fn empty_image() {
        assert!(debayer(&[1.0], 1, 1, 2, &[0, 1, 1, 2]).is_empty());
    }
}
