//! As-shot white balance in camera space.

use crate::node::{EvalContext, Evaluated, InputSpec, NodeKind, OutputSpec};
use crate::param::Params;
use crate::value::{Mosaic, PortType, Value};
use rayon::prelude::*;
use std::sync::Arc;

/// Multiplies each site by the camera's as-shot multiplier for its color.
pub static WHITE_BALANCE: NodeKind = NodeKind {
    name: "color.white_balance",
    label: "white balance",
    params: &[],
    inputs: &[InputSpec { name: "mosaic", accepts: &[PortType::Mosaic] }],
    outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
    eval: white_balance,
    actions: &[],
};

fn white_balance(_: Params, inputs: &[Value], _: &EvalContext) -> Result<Evaluated, String> {
    let m = inputs[0].mosaic();
    let wb = m.camera.white_balance;
    let gains: Vec<_> = m.cfa.colors.iter().map(|&c| wb[c as usize]).collect();
    let data = process(&m.data, m.width, m.cfa.size, &gains);
    super::single(Value::Mosaic(Arc::new(Mosaic {
        data,
        white: std::array::from_fn(|c| m.white[c] * wb[c]),
        camera: m.camera.clone(),
        cfa: m.cfa.clone(),
        ..**m
    })))
}

pub(super) fn process(input: &[f32], width: usize, cfa_size: usize, gains: &[f32]) -> Vec<f32> {
    input
        .par_iter()
        .enumerate()
        .map(|(i, &value)| {
            let phase = (i / width % cfa_size) * cfa_size + i % width % cfa_size;
            value * gains[phase]
        })
        .collect()
}
