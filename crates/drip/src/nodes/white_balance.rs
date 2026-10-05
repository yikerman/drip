//! As-shot white balance in camera space.

use crate::image::Mosaic;
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::Params;
use crate::ports::Read;
use rayon::prelude::*;
use std::sync::Arc;

/// Multiplies each site by the camera's as-shot multiplier for its color.
pub static WHITE_BALANCE: NodeKind = NodeKind::new::<WhiteBalance>(
    "color.white_balance",
    "white balance",
    &[],
    &["mosaic"],
    &["mosaic"],
);

struct WhiteBalance;
impl NodeKernel for WhiteBalance {
    type Inputs = (Read<Mosaic>,);
    type Outputs = (Arc<Mosaic>,);
    fn eval(
        _: Params<'_>,
        (m,): (&Mosaic,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        let wb = m.camera.white_balance;
        let gains: Vec<_> = m.cfa.colors.iter().map(|&c| wb[c as usize]).collect();
        let data = process(&m.data, m.width, m.cfa.size, &gains);
        Ok(Evaluated::new((Arc::new(Mosaic {
            data,
            white: std::array::from_fn(|c| m.white[c] * wb[c]),
            camera: m.camera.clone(),
            cfa: m.cfa.clone(),
            ..*m
        }),)))
    }
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
