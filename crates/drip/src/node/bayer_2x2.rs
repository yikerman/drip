use super::reduce;
use crate::registry::tag;
use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
fn bin2x2_contract(global: &GlobalContext, _: &(), image: &BayerDesc) -> Result<(RgbDesc,)> {
    if image.extent.width < 2 || image.extent.height < 2 {
        return Err(Error::Contract("Bayer 2x2 demosaic needs at least 2x2 samples".into()));
    }
    let extent = reduce::demosaic_extent(global, &image.extent);
    Ok((RgbDesc {
        extent: Extent { width: extent.width / 2, height: extent.height / 2 },
        coordinates: Coordinates::Camera(image.response.clone()),
    },))
}
/// Demosaic Bayer by averaging each 2×2 cell into camera RGB, including both green
/// sites. Requires an explicit Bayer phase and drops incomplete edge cells.
/// Requested preview detail first reduces each CFA phase plane.
#[crate::node(id = "bayer-2x2-demosaic", name="Bayer 2×2 demosaic", tags = [tag::INPUT_TRANSFORM, tag::BAYER_RAW, tag::DEMOSAIC], contract = bin2x2_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Bayer>>,
    output: Write<'_, Device<Rgb>, CameraRgb>,
) -> Result<()> {
    let reduced = reduce::for_demosaic(ctx, &image)?;
    let image = match &reduced {
        Some((desc, data)) => Read::new(desc, data),
        None => image,
    };

    let (count, dim) = dispatch_dims(output.desc.extent.pixels());
    kernel::bin2x2::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        output.data.argument(),
        image.desc.extent.width as usize,
        image.desc.extent.height as usize,
        image.desc.phase as usize,
    );
    Ok(())
}

mod kernel {
    // SPDX-License-Identifier: GPL-3.0-or-later
    // Source pins and numerical deviations: THIRD_PARTY.md.
    use crate::node::shared_kernel::color;
    use cubecl::prelude::*;
    #[cube(launch)]
    pub fn bin2x2(input: &[f32], output: &mut [f32], width: usize, height: usize, phase: usize) {
        let i = ABSOLUTE_POS;
        let ow = width / 2;
        if i < ow * (height / 2) {
            let row = i / ow * 2;
            let col = i % ow * 2;
            let mut rgb = Array::<f32>::new(3usize);
            for c in 0..3 {
                rgb[c] = 0.0f32;
            }
            for y in 0..2 {
                for x in 0..2 {
                    let c = color(row + y, col + x, phase);
                    rgb[c] += input[(row + y) * width + col + x];
                }
            }
            output[i * 3] = rgb[0];
            output[i * 3 + 1] = rgb[1] * 0.5f32;
            output[i * 3 + 2] = rgb[2];
        }
    }
}
