//! Sensor-space highlight repair before preview reduction and demosaicing.

use crate::node::{InputSpec, NodeKind, OutputSpec};
use crate::param::{ParamKind, ParamSpec};
use crate::value::{Mosaic, PortType, Value};
use std::sync::Arc;
mod opposed;

pub static HIGHLIGHTS: NodeKind = NodeKind {
    name: "raw.highlights",
    label: "highlights",
    params: &[ParamSpec::new("threshold", ParamKind::Float { min: 0.5, max: 1.0, default: 0.98 })],
    inputs: &[InputSpec { name: "mosaic", accepts: &[PortType::Mosaic] }],
    outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
    eval: |p, inputs, _| {
        let input = inputs[0].mosaic();
        let data = opposed::process(input, p.float("threshold") as f32);
        super::single(Value::Mosaic(Arc::new(Mosaic {
            data,
            cfa: input.cfa.clone(),
            camera: input.camera.clone(),
            ..**input
        })))
    },
    actions: &[],
};
