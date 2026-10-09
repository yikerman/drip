use super::reduce;
use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
fn bayer_preview_contract(
    global: &GlobalContext,
    _: &(),
    image: Option<&BayerDesc>,
) -> Result<(Option<ImageDesc<Camera>>,)> {
    let output = image
        .map(|d| {
            let extent = reduce::demosaic_extent(global, &d.extent);
            if d.extent.width < 2 || d.extent.height < 2 {
                return Err(Error::Contract("Bayer 2x2 preview needs at least 2x2 samples".into()));
            }
            Ok(ImageDesc {
                extent: Extent { width: extent.width / 2, height: extent.height / 2 },
                interpretation: d.interpretation.clone(),
            })
        })
        .transpose()?;
    Ok((output,))
}
/// Average each Bayer 2x2 cell into CameraRgb, averaging the two green samples.
/// Global scale first reduces its four CFA phase planes. Drops incomplete edge
/// cells. Requires declared Bayer phase; no CFA guessing.
#[crate::node(id = "bayer-preview-2x2", name="Bayer 2×2", category="demosaic", contract = bayer_preview_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Bayer>>,
    output: Write<'_, Device<CameraRgb>>,
) -> Result<()> {
    let reduced = reduce::for_demosaic(ctx, &image)?;
    let image = match &reduced {
        Some((desc, data)) => Read { desc, data },
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
            output[i * 4] = rgb[0];
            output[i * 4 + 1] = rgb[1] * 0.5f32;
            output[i * 4 + 2] = rgb[2];
            output[i * 4 + 3] = 0.0f32;
        }
    }
}
