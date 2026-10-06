//! Camera characterization to linear Rec.2020.

use crate::color::{self, D65, REC2020};
use crate::image::{CameraRgb, Rgb, SceneRec2020, ThreeChannelMatrix};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::Params;
use crate::ports::Read;
use rayon::prelude::*;
use std::sync::Arc;

pub static CAMERA_TO_REC2020: NodeKind = NodeKind::new::<CameraToRec2020>(
    "color.camera_to_rec2020",
    "color",
    "Camera to Rec.2020",
    &[],
    &["image"],
    &["image"],
);

struct CameraToRec2020;
impl NodeKernel for CameraToRec2020 {
    type Inputs = (Read<CameraRgb>,);
    type Outputs = (Arc<SceneRec2020>,);
    fn eval(
        _: Params<'_>,
        (camera_rgb,): (&CameraRgb,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let image = camera_rgb.rgb();
        let camera = &camera_rgb.camera;
        let m = color::camera_to_rgb(
            &color::to_f64(&camera.xyz_to_cam),
            &color::rgb_to_xyz(REC2020, D65),
        );
        let m = color::to_f32(&m.expect("raw.read rejects degenerate matrices"));
        let pixels = matrix(&image.pixels, &m);
        Ok(Evaluated::new((Arc::new(SceneRec2020::from(Arc::new(Rgb { pixels, ..**image }))),)))
    }
}

pub fn matrix(input: &[[f32; 3]], matrix: &[[f32; 3]; 3]) -> Vec<[f32; 3]> {
    input
        .par_iter()
        .map(|&[r, g, b]| matrix.map(|row| row[0] * r + row[1] * g + row[2] * b))
        .collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn concurrent_evaluations_keep_their_buffers_independent() {
        std::thread::scope(|scope| {
            for worker in 0..4 {
                scope.spawn(move || {
                    let matrix = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
                    for round in 0..8 {
                        let value = (worker * 8 + round) as f32;
                        let input = vec![[value; 3]; 4097 + round];
                        let output: Vec<[f32; 3]> = super::matrix(&input, &matrix);
                        assert!(
                            output
                                .iter()
                                .all(|&pixel| pixel == [value * 2.0, value * 3.0, value * 4.0])
                        );
                    }
                });
            }
        });
    }

    #[test]
    fn matrix_matches_f64_reference_at_uneven_lengths() {
        let matrix = [[1.2, -0.1, -0.1], [-0.3, 1.5, -0.2], [0.0, -0.1, 1.1]];
        for len in [1, 17, 4097] {
            let input: Vec<_> = (0..len)
                .map(|i| std::array::from_fn(|c| ((i * 3 + c) * 37 % 1009) as f32 / 71.0 - 2.0))
                .collect();
            let output: Vec<[f32; 3]> = super::matrix(&input, &matrix);
            for (pixel, source) in output.iter().zip(&input) {
                for (value, row) in pixel.iter().zip(matrix) {
                    let expected: f64 =
                        row.iter().zip(source).map(|(&a, &b)| f64::from(a) * f64::from(b)).sum();
                    assert!((f64::from(*value) - expected).abs() < 4e-6);
                }
            }
        }
    }
}
