//! UI-only nodes: no outputs, only views for frontends to present (DESIGN U1).

use std::sync::Arc;

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind};
use crate::param::Params;
use crate::value::{Histogram, PortType, Value, View};

use super::kernels::{self, BINS};

/// Shows a Rec.2020 image; the frontend handles the display transform.
pub static PREVIEW: NodeKind = NodeKind {
    name: "view.preview",
    label: "preview",
    params: &[],
    inputs: &[InputSpec {
        name: "image",
        accepts: &[PortType::SceneRec2020, PortType::DisplayRec2020],
    }],
    outputs: &[],
    eval: |_, inputs, _| {
        Ok(Evaluated { outputs: vec![], view: Some(View::Image(inputs[0].clone())) })
    },
    actions: &[],
};

pub static HISTOGRAM: NodeKind = NodeKind {
    name: "view.histogram",
    label: "histogram",
    params: &[],
    inputs: &[InputSpec {
        name: "image",
        accepts: &[PortType::CameraRgb, PortType::SceneRec2020, PortType::DisplayRec2020],
    }],
    outputs: &[],
    eval: histogram,
    actions: &[],
};

const STOPS: (f32, f32) = (-12.0, 4.0);

fn histogram(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let thresholds =
        std::array::from_fn(|i| 2f32.powf(STOPS.0 + i as f32 * (STOPS.1 - STOPS.0) / BINS as f32));
    let counts = kernels::histogram(&inputs[0].rgb().pixels, &thresholds);
    let histogram = Histogram { min_stop: STOPS.0, max_stop: STOPS.1, counts };
    Ok(Evaluated { outputs: vec![], view: Some(View::Histogram(Arc::new(histogram))) })
}
