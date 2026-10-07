//! As-shot white balance in camera space.

use crate::image::Mosaic;
use crate::node::{EvalContext, KernelError};
use rayon::prelude::*;
use std::sync::Arc;

/// Multiply Bayer samples by the as-shot gains relative to the first green channel.
///
/// Apply once before highlight reconstruction.
#[crate::node(kind = WHITE_BALANCE, id = "color.white_balance", category = "color", name = "White balance", outputs = ["mosaic"])]
fn white_balance(
    _: (),
    (mosaic,): (&Mosaic,),
    _: &EvalContext<'_>,
) -> Result<(Arc<Mosaic>,), KernelError> {
    let wb = mosaic.interpretation().camera.white_balance;
    let gains: Vec<_> =
        mosaic.interpretation().cfa.colors.iter().map(|&c| wb[c as usize]).collect();
    let data = process(mosaic.samples(), mosaic.width, mosaic.interpretation().cfa.size, &gains);
    Ok((Arc::new(Mosaic::new(
        Arc::new(crate::image::RawMat::from_samples(
            mosaic.width,
            mosaic.height,
            mosaic.scale,
            data,
        )),
        crate::image::SensorMosaic {
            cfa: mosaic.interpretation().cfa.clone(),
            white: std::array::from_fn(|c| mosaic.interpretation().white[c] * wb[c]),
            camera: mosaic.interpretation().camera.clone(),
        },
    )),))
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
