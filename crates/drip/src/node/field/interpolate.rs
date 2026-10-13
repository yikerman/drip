use super::{Rect, RgbField, RgbFieldDesc};
use crate::{
    Result,
    node::data::*,
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext},
};
pub use lift::{Definition, Ports, add, definition};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbFieldDesc,)> {
    let full = [f64::from(image.full.width), f64::from(image.full.height)];
    Ok((RgbFieldDesc {
        source: image.clone(),
        domain: Rect { origin: [0.0, 0.0], size: full },
        maps: Default::default(),
    },))
}

/// Treat RGB samples as a continuous image (RGB field) for geometry nodes.
/// Nothing is computed: the field shares the samples, defined over the
/// image's full-detail extent, and a later Sample reconstructs and filters
/// them once, however many transforms come between. Coordinates are kept.
#[crate::node(id = "interpolate", name = "Interpolate", tags = [tag::GEOMETRY], contract = contract)]
pub fn lift(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Rgb>>,
    output: Forward<'_, Device<RgbField>>,
) -> Result<()> {
    output.set(image.data.share_as());
    Ok(())
}
