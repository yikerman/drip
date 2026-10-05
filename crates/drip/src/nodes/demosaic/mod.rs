//! Full-size Bayer demosaicing. Preview reduction belongs immediately before
//! interpolation, after sensor-space processing such as highlight reconstruction.

use crate::image::{CameraRgb, Mosaic, Rgb};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::Params;
use crate::ports::Read;
use std::sync::Arc;
mod rcd;

pub static RCD: NodeKind =
    NodeKind::new::<RcdDemosaic>("demosaic.rcd", "demosaic", &[], &["mosaic"], &["image"]);

struct RcdDemosaic;
impl NodeKernel for RcdDemosaic {
    type Inputs = (Read<Mosaic>,);
    type Outputs = (Arc<CameraRgb>,);
    fn eval(
        _: Params<'_>,
        (input,): (&Mosaic,),
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let mosaic = preview(input, ctx);
        let pixels = rcd::process(&mosaic);
        let image = Rgb { width: mosaic.width, height: mosaic.height, scale: mosaic.scale, pixels };
        Ok(Evaluated::new((Arc::new(CameraRgb {
            data: Arc::new(image),
            camera: mosaic.camera.clone(),
        }),)))
    }
}

/// Borrow full detail, allocate only when a lower-resolution mosaic is needed.
pub(super) fn preview<'a>(input: &'a Mosaic, ctx: &EvalContext) -> std::borrow::Cow<'a, Mosaic> {
    let mut mosaic = std::borrow::Cow::Borrowed(input);
    while mosaic.scale < ctx.scale() {
        mosaic = std::borrow::Cow::Owned(super::downsample(&mosaic));
    }
    mosaic
}
