//! UI-only nodes: no outputs, only views for frontends to present (DESIGN U1).

use std::sync::Arc;

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind};
use crate::param::Params;
use crate::value::{Histogram, PortType, Value, View};

/// Shows a Rec.2020 image; the frontend handles the display transform.
pub static PREVIEW: NodeKind = NodeKind {
    name: "view.preview",
    version: 1,
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
    migrate: None,
};

pub static HISTOGRAM: NodeKind = NodeKind {
    name: "view.histogram",
    version: 1,
    params: &[],
    inputs: &[InputSpec {
        name: "image",
        accepts: &[PortType::CameraRgb, PortType::SceneRec2020, PortType::DisplayRec2020],
    }],
    outputs: &[],
    eval: histogram,
    actions: &[],
    migrate: None,
};

const BINS: usize = 256;
const STOPS: (f32, f32) = (-12.0, 4.0);

fn histogram(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let mut counts = vec![[0; 3]; BINS];
    let per_stop = BINS as f32 / (STOPS.1 - STOPS.0);
    for pixel in &inputs[0].rgb().pixels {
        for (c, v) in pixel.iter().enumerate() {
            // log2 gives -inf for 0 and NaN for negatives; both cast to bin 0.
            let bin = ((v.log2() - STOPS.0) * per_stop) as usize;
            counts[bin.min(BINS - 1)][c] += 1;
        }
    }
    let histogram = Histogram { min_stop: STOPS.0, max_stop: STOPS.1, counts };
    Ok(Evaluated { outputs: vec![], view: Some(View::Histogram(Arc::new(histogram))) })
}
