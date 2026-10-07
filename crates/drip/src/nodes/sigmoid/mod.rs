//! Creative tone mapping in linear Rec.2020; algorithm settings also serve frontend curve plots.

use crate::image::{Rec2020Mat, Rec2020Rgb, Rgb};
use crate::node::{EvalContext, Evaluated, KernelError};
use crate::param::ParamKind;
use crate::ports::MatRef;
use std::sync::Arc;

mod algorithm;
pub use algorithm::{GREY, Sigmoid};

#[derive(crate::Parameters)]
pub struct Settings {
    #[param(ParamKind::Float { min: 0.5, max: 4.0, default: 1.5 })]
    contrast: f32,
    #[param(ParamKind::Float { min: -1.0, max: 1.0, default: -0.2 })]
    skew: f32,
    #[param(ParamKind::Float { min: 0.0, max: 1.0, default: 0.0 })]
    preserve_hue: f32,
}
impl Settings {
    pub fn curve(&self) -> Sigmoid {
        Sigmoid::new(self.contrast, self.skew, self.preserve_hue)
    }
}

/// Creative S-curve on linear Rec.2020.
///
/// Assumes middle grey at 0.18 and keeps it fixed. Black is 0 and the curve approaches 1.
/// The output is interpreted as linear Rec.2020 for further processing; additional
/// input guarantees are dropped. Preview and export do not require tone mapping.
#[crate::node(kind = SIGMOID, id = "tone.sigmoid", category = "tone", name = "Sigmoid", outputs = ["image"], references = [("darktable: sigmoid", "https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/sigmoid/")])]
fn sigmoid(
    p: Settings,
    (image,): (MatRef<'_, 3, dyn Rec2020Rgb>,),
    _: &EvalContext<'_>,
) -> Result<Evaluated<(Arc<Rec2020Mat>,)>, KernelError> {
    let input = image.rgb();
    let pixels = p.curve().process(&input.pixels);
    Ok(Evaluated {
        outputs: (Arc::new(Rec2020Mat::from(Arc::new(Rgb { pixels, ..**input }))),),
        view: (),
    })
}
