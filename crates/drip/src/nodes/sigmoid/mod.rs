//! Scene-to-display mapping; algorithm settings also serve frontend curve plots.

use crate::node::{Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::value::{PortType, Rgb, Value};
use std::sync::Arc;

mod algorithm;
pub use algorithm::{GREY, Sigmoid};

pub static SIGMOID: NodeKind = NodeKind {
    name: "tone.sigmoid",
    label: "sigmoid",
    params: &[
        ParamSpec::new("contrast", ParamKind::Float { min: 0.5, max: 4.0, default: 1.5 }),
        ParamSpec::new("skew", ParamKind::Float { min: -1.0, max: 1.0, default: -0.2 }),
        ParamSpec::new("preserve_hue", ParamKind::Float { min: 0.0, max: 1.0, default: 0.0 }),
    ],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::SceneRec2020] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::DisplayRec2020 }],
    eval: |p, inputs, _| {
        let input = inputs[0].rgb();
        let pixels = settings(p).process(&input.pixels);
        Ok(Evaluated {
            outputs: vec![Value::DisplayRec2020(Arc::new(Rgb { pixels, ..**input }))],
            view: None,
        })
    },
    actions: &[],
};

pub fn settings(p: Params<'_>) -> Sigmoid {
    Sigmoid::new(p.float("contrast") as f32, p.float("skew") as f32, p.float("preserve_hue") as f32)
}
