//! Sensor-space highlight repair before preview reduction and demosaicing.

use crate::image::Mosaic;
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::ports::Read;
use std::sync::Arc;
mod opposed;

pub static HIGHLIGHTS: NodeKind = NodeKind::new::<ReconstructHighlights>(
    "raw.highlights",
    "raw",
    "Highlights",
    &[ParamSpec::new("threshold", ParamKind::Float { min: 0.5, max: 1.0, default: 0.98 })],
    &["mosaic"],
    &["mosaic"],
);

struct ReconstructHighlights;
impl NodeKernel for ReconstructHighlights {
    type Inputs = (Read<Mosaic>,);
    type Outputs = (Arc<Mosaic>,);
    fn eval(
        p: Params<'_>,
        (input,): (&Mosaic,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let data = opposed::process(input, p.float("threshold") as f32);
        Ok(Evaluated::new((Arc::new(Mosaic {
            data,
            cfa: input.cfa.clone(),
            camera: input.camera.clone(),
            ..*input
        }),)))
    }
}
