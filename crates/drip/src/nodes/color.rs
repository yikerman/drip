//! Camera-space color processing: white balance, debayering, and conversion
//! to Rec.2020 (DESIGN C1, C3, C6).

use std::sync::Arc;

use crate::color::{self, D65, REC2020};
use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::Params;
use crate::value::{Mosaic, PortType, Rgb, Value};

const MOSAIC: &[PortType] = &[PortType::Mosaic];

fn single(output: Value) -> Result<Evaluated, String> {
    Ok(Evaluated { outputs: vec![output], view: None })
}

/// Multiplies each site by the camera's as-shot multiplier for its color.
pub static WHITE_BALANCE: NodeKind = NodeKind {
    name: "color.white_balance",
    version: 1,
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: MOSAIC }],
    outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
    eval: white_balance,
    actions: &[],
    migrate: None,
};

fn white_balance(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let m = inputs[0].mosaic();
    let wb = m.camera.white_balance;
    let data = (0..m.height)
        .flat_map(|row| (0..m.width).map(move |col| (row, col)))
        .map(|(row, col)| m.data[row * m.width + col] * wb[m.cfa.color(row, col) as usize])
        .collect();
    single(Value::Mosaic(Arc::new(Mosaic {
        data,
        camera: m.camera.clone(),
        cfa: m.cfa.clone(),
        ..**m
    })))
}

/// Naive debayering: each 2 × 2 Bayer cell becomes one pixel, averaging its
/// two greens. Halves the resolution (DESIGN C6).
pub static BIN_2X2: NodeKind = NodeKind {
    name: "demosaic.bin2x2",
    version: 1,
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: MOSAIC }],
    outputs: &[OutputSpec { name: "image", ty: PortType::CameraRgb }],
    eval: bin_2x2,
    actions: &[],
    migrate: None,
};

fn bin_2x2(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let m = inputs[0].mosaic();
    let (width, height) = (m.width / 2, m.height / 2);
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (y, x)))
        .map(|(y, x)| {
            let (mut sum, mut count) = ([0.0; 3], [0.0; 3]);
            for (row, col) in [(0, 0), (0, 1), (1, 0), (1, 1)].map(|(r, c)| (2 * y + r, 2 * x + c))
            {
                // The second green (3) joins the first.
                let channel = [0, 1, 2, 1][m.cfa.color(row, col) as usize];
                sum[channel] += m.data[row * m.width + col];
                count[channel] += 1.0;
            }
            std::array::from_fn(|c| sum[c] / count[c])
        })
        .collect();
    single(Value::CameraRgb(
        Arc::new(Rgb { width, height, scale: m.scale * 2, pixels }),
        m.camera.clone(),
    ))
}

pub static CAMERA_TO_REC2020: NodeKind = NodeKind {
    name: "color.camera_to_rec2020",
    version: 1,
    params: &[],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::CameraRgb] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: camera_to_rec2020,
    actions: &[],
    migrate: None,
};

fn camera_to_rec2020(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let Value::CameraRgb(image, camera) = &inputs[0] else {
        unreachable!("ports guarantee camera RGB")
    };
    let m =
        color::camera_to_rgb(&color::to_f64(&camera.xyz_to_cam), &color::rgb_to_xyz(REC2020, D65));
    let m = color::to_f32(&m.expect("raw.read rejects degenerate matrices"));
    single(Value::SceneRec2020(Arc::new(image.map(|p| color::apply_f32(&m, p)))))
}
