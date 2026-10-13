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
    let half = |e: &Extent| Extent { width: e.width / 2, height: e.height / 2 };
    Ok((RgbDesc {
        extent: half(&extent),
        full: half(&image.extent),
        window: None,
        coordinates: Coordinates::Camera(image.response.clone()),
    },))
}
/// Demosaic Bayer data to camera RGB at half width and height by averaging
/// each 2×2 cell, both greens included. It is fast and free of interpolation
/// artifacts. An incomplete last row or column of cells is dropped.
// At a preview scale the kernel first reduces each CFA phase plane.
#[crate::node(id = "bayer-2x2-demosaic", name="Bayer 2×2 demosaic", tags = [tag::INPUT_TRANSFORM, tag::BAYER_RAW, tag::DEMOSAIC], contract = bin2x2_contract, scaled)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Bayer>,
    output: Write<'_, Rgb, CameraRgb>,
) -> Result<()> {
    let reduced = reduce::for_demosaic(ctx, image.desc, image.data.device(ctx)?)?;
    let image = match &reduced {
        Some(raster) => Read::new(raster),
        None => image,
    };

    let (count, dim) = dispatch_dims(output.desc.extent.pixels());
    kernel::bin2x2::launch(
        ctx.client()?,
        count,
        dim,
        image.data.device(ctx)?.argument(),
        output.device(ctx)?.argument(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_the_full_detail_extent() {
        let response = Response {
            camera: Camera { make: "test".into(), model: "camera".into() },
            balance: None,
        };
        let image = BayerDesc {
            extent: Extent { width: 6025, height: 4016 },
            phase: BayerPhase::Rggb,
            response,
        };
        let (preview,) = bin2x2_contract(&GlobalContext { scale: 8 }, &(), &image).unwrap();
        assert_eq!(preview.extent, Extent { width: 376, height: 251 });
        assert_eq!(preview.full, Extent { width: 3012, height: 2008 });
    }
}
