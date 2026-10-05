//! Tone mapping from scene- to display-referred values (DESIGN C4).

use std::sync::Arc;

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::value::{PortType, Rgb, Value};

use super::kernels;

/// Scene middle grey, mapped to the same display value.
const GREY: f32 = 0.18;

/// Per-channel sigmoid `y = x^c / (x^c + k)` on exposed scene values `x`: the
/// Michaelis-Menten/Naka-Rushton form [1], with `k` chosen so middle grey
/// stays put. It maps [0, ∞) onto [0, 1), and `c` sets the contrast around
/// grey. Per-channel curves desaturate and shift hue in bright saturated areas.
///
/// [1] K.-I. Naka and W. A. H. Rushton, "S-potentials from colour units in the
///     retina of fish (Cyprinidae)," J. Physiol., vol. 185, no. 3,
///     pp. 536-555, 1966.
pub static SIGMOID: NodeKind = NodeKind {
    name: "tone.sigmoid",
    label: "sigmoid",
    params: &[
        ParamSpec::new("exposure", ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 }),
        ParamSpec::new("contrast", ParamKind::Float { min: 0.5, max: 4.0, default: 1.5 }),
    ],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::SceneRec2020] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::DisplayRec2020 }],
    eval: sigmoid,
    actions: &[],
};

fn sigmoid(p: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let (exposure, c) = (p.float("exposure") as f32, p.float("contrast") as f32);
    let k = GREY.powf(c) * (1.0 / GREY - 1.0);
    let offset = c * exposure * std::f32::consts::LN_2 - k.ln();
    let input = inputs[0].rgb();
    let pixels = kernels::sigmoid(&input.pixels, c, offset);
    let image = Rgb { pixels, ..**input };
    Ok(Evaluated { outputs: vec![Value::DisplayRec2020(Arc::new(image))], view: None })
}
