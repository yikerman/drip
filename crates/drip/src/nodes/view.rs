//! UI-only nodes: no outputs, only views for frontends to present (DESIGN U1).

use std::sync::Arc;

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind};
use crate::param::{ParamKind, ParamSpec, Params};
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

/// Counts stops from `min_ev` to `max_ev`, for plotting on a linear or log
/// `scale`. The bounds lie on either side of 0 EV, so the range is never empty
/// and always shows where 1.0 falls.
pub static HISTOGRAM: NodeKind = NodeKind {
    name: "view.histogram",
    label: "histogram",
    params: &[
        ParamSpec::new("min_ev", ParamKind::Int { min: -24, max: -1, default: -12 }),
        ParamSpec::new("max_ev", ParamKind::Int { min: 1, max: 10, default: 4 }),
        ParamSpec::new("scale", ParamKind::Choice { options: &["linear", "log"], default: "log" }),
    ],
    inputs: &[InputSpec {
        name: "image",
        accepts: &[PortType::CameraRgb, PortType::SceneRec2020, PortType::DisplayRec2020],
    }],
    outputs: &[],
    eval: histogram,
    actions: &[],
};

fn histogram(p: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let (min, max) = (p.int("min_ev") as f32, p.int("max_ev") as f32);
    let thresholds = std::array::from_fn(|i| 2f32.powf(min + i as f32 * (max - min) / BINS as f32));
    let counts = kernels::histogram(&inputs[0].rgb().pixels, &thresholds);
    let log = p.choice("scale") == "log";
    let histogram = Histogram { min_stop: min, max_stop: max, counts, log };
    Ok(Evaluated { outputs: vec![], view: Some(View::Histogram(Arc::new(histogram))) })
}
