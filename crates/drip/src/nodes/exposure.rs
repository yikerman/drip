//! Scene-linear exposure, independent of the display transform.

use crate::node::{InputSpec, NodeKind, OutputSpec};
use crate::param::{ParamKind, ParamSpec};
use crate::value::{PortType, Rgb, Value};
use rayon::prelude::*;
use std::sync::Arc;

pub static EXPOSURE: NodeKind = NodeKind {
    name: "color.exposure",
    label: "exposure",
    params: &[ParamSpec::new("ev", ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 })],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::SceneRec2020] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: |p, inputs, _| {
        let image = inputs[0].rgb();
        let gain = (p.float("ev") as f32).exp2();
        let pixels = image.pixels.par_iter().map(|p| p.map(|v| v * gain)).collect();
        super::single(Value::SceneRec2020(Arc::new(Rgb { pixels, ..**image })))
    },
    actions: &[],
};
