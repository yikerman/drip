//! Image presentation without processing side effects.

use crate::node::{Evaluated, InputSpec, NodeKind};
use crate::value::{PortType, View};

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
