//! Full-size Bayer demosaicing. Preview reduction belongs immediately before
//! interpolation, after sensor-space processing such as highlight reconstruction.

use crate::node::{EvalContext, InputSpec, NodeKind, OutputSpec};
use crate::value::{Mosaic, PortType, Rgb, Value};
use std::sync::Arc;
mod rcd;

pub static RCD: NodeKind = NodeKind {
    name: "demosaic.rcd",
    label: "demosaic",
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: &[PortType::Mosaic] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::CameraRgb }],
    eval: |_, inputs, ctx| {
        let mosaic = preview(inputs[0].mosaic(), ctx);
        let pixels = rcd::process(&mosaic);
        let image = Rgb { width: mosaic.width, height: mosaic.height, scale: mosaic.scale, pixels };
        super::single(Value::CameraRgb(Arc::new(image), mosaic.camera.clone()))
    },
    actions: &[],
};

/// Borrow full detail, allocate only when a lower-resolution mosaic is needed.
pub(super) fn preview<'a>(input: &'a Mosaic, ctx: &EvalContext) -> std::borrow::Cow<'a, Mosaic> {
    let mut mosaic = std::borrow::Cow::Borrowed(input);
    while mosaic.scale < ctx.scale() {
        mosaic = std::borrow::Cow::Owned(super::downsample(&mosaic));
    }
    mosaic
}
