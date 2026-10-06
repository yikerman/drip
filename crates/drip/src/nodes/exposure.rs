//! Scene-linear exposure, independent of the display transform.

use crate::image::{Rgb, SceneRec2020, ThreeChannelMatrix};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::ports::Read;
use rayon::prelude::*;
use std::sync::Arc;

pub static EXPOSURE: NodeKind = NodeKind::new::<Exposure>(
    "color.exposure",
    "color",
    "Exposure",
    &[ParamSpec::new("ev", ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 })],
    &["image"],
    &["image"],
);

struct Exposure;
impl NodeKernel for Exposure {
    type Inputs = (Read<SceneRec2020>,);
    type Outputs = (Arc<SceneRec2020>,);
    fn eval(
        p: Params<'_>,
        (input,): (&SceneRec2020,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let image = input.rgb();
        let gain = (p.float("ev") as f32).exp2();
        let pixels = image.pixels.par_iter().map(|p| p.map(|v| v * gain)).collect();
        Ok(Evaluated::new((Arc::new(SceneRec2020::from(Arc::new(Rgb { pixels, ..**image }))),)))
    }
}
