//! Scene-to-display mapping; algorithm settings also serve frontend curve plots.

use crate::image::{DisplayRec2020, Rgb, SceneRec2020, ThreeChannelMatrix};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::ports::Read;
use std::sync::Arc;

mod algorithm;
pub use algorithm::{GREY, Sigmoid};

pub static SIGMOID: NodeKind = NodeKind::new::<SigmoidNode>(
    "tone.sigmoid",
    "tone",
    "Sigmoid",
    &[
        ParamSpec::new("contrast", ParamKind::Float { min: 0.5, max: 4.0, default: 1.5 }),
        ParamSpec::new("skew", ParamKind::Float { min: -1.0, max: 1.0, default: -0.2 }),
        ParamSpec::new("preserve_hue", ParamKind::Float { min: 0.0, max: 1.0, default: 0.0 }),
    ],
    &["image"],
    &["image"],
);

struct SigmoidNode;
impl NodeKernel for SigmoidNode {
    type Inputs = (Read<SceneRec2020>,);
    type Outputs = (Arc<DisplayRec2020>,);
    fn eval(
        p: Params<'_>,
        (image,): (&SceneRec2020,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let input = image.rgb();
        let pixels = settings(p).process(&input.pixels);
        Ok(Evaluated {
            outputs: (Arc::new(DisplayRec2020::from(Arc::new(Rgb { pixels, ..**input }))),),
            view: None,
        })
    }
}

pub fn settings(p: Params<'_>) -> Sigmoid {
    Sigmoid::new(p.float("contrast") as f32, p.float("skew") as f32, p.float("preserve_hue") as f32)
}
