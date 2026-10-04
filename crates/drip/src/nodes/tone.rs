//! Tone mapping from scene- to display-referred values (DESIGN C4).

use std::sync::Arc;

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::value::{PortType, Value};

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
    version: 1,
    params: &[
        ParamSpec {
            name: "exposure",
            kind: ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 },
        },
        ParamSpec { name: "contrast", kind: ParamKind::Float { min: 0.5, max: 4.0, default: 1.5 } },
    ],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::SceneRec2020] }],
    outputs: &[OutputSpec { name: "image", ty: PortType::DisplayRec2020 }],
    eval: sigmoid,
    actions: &[],
    migrate: None,
};

fn sigmoid(p: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let (gain, c) = (2f32.powf(p.float("exposure") as f32), p.float("contrast") as f32);
    let k = GREY.powf(c) * (1.0 / GREY - 1.0);
    // x / (x + k) rearranged so that x = ∞ gives 1 rather than ∞ / ∞.
    let curve = |v: f32| 1.0 / (1.0 + k / (v * gain).max(0.0).powf(c));
    let image = inputs[0].rgb().map(|p| p.map(curve));
    Ok(Evaluated { outputs: vec![Value::DisplayRec2020(Arc::new(image))], view: None })
}
