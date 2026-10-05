//! Camera-space color processing: white balance, debayering, and conversion
//! to Rec.2020 (DESIGN C1, C3, C6).

use std::sync::Arc;

use crate::color::{self, D65, REC2020};
use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::Params;
use crate::value::{Mosaic, PortType, Rgb, Value};

use super::kernels;

const MOSAIC: &[PortType] = &[PortType::Mosaic];

fn single(output: Value) -> Result<Evaluated, String> {
    Ok(Evaluated { outputs: vec![output], view: None })
}

/// Multiplies each site by the camera's as-shot multiplier for its color.
pub static WHITE_BALANCE: NodeKind = NodeKind {
    name: "color.white_balance",
    label: "white balance",
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: MOSAIC }],
    outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
    eval: white_balance,
    actions: &[],
};

fn white_balance(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let m = inputs[0].mosaic();
    let wb = m.camera.white_balance;
    let gains: Vec<_> = m.cfa.colors.iter().map(|&c| wb[c as usize]).collect();
    let data = kernels::white_balance(&m.data, m.width, m.cfa.size, &gains);
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
    label: "debayer",
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: MOSAIC }],
    outputs: &[OutputSpec { name: "image", ty: PortType::CameraRgb }],
    eval: bin_2x2,
    actions: &[],
};

fn bin_2x2(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let m = inputs[0].mosaic();
    let (width, height) = (m.width / 2, m.height / 2);
    // The second green (3) joins the first.
    let colors: Vec<u32> = m.cfa.colors.iter().map(|&c| [0, 1, 2, 1][c as usize]).collect();
    let pixels = kernels::debayer(&m.data, m.width, m.height, m.cfa.size, &colors);
    single(Value::CameraRgb(
        Arc::new(Rgb { width, height, scale: m.scale * 2, pixels }),
        m.camera.clone(),
    ))
}

pub static CAMERA_TO_REC2020: NodeKind = NodeKind {
    name: "color.camera_to_rec2020",
    label: "camera to rec2020",
    params: &[],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::CameraRgb] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: camera_to_rec2020,
    actions: &[],
};

fn camera_to_rec2020(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let Value::CameraRgb(image, camera) = &inputs[0] else {
        unreachable!("ports guarantee camera RGB")
    };
    let m =
        color::camera_to_rgb(&color::to_f64(&camera.xyz_to_cam), &color::rgb_to_xyz(REC2020, D65));
    let m = color::to_f32(&m.expect("raw.read rejects degenerate matrices"));
    let pixels = kernels::matrix(&image.pixels, &m);
    single(Value::SceneRec2020(Arc::new(Rgb { pixels, ..**image })))
}
