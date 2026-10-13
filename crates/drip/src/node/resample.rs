//! Resample: the explicit materialization of an image on a new lattice,
//! with a chosen filter and size. It has no kernel of its own. Evaluation
//! composes it into the fused resampling stage (`plan.rs`).
use crate::{
    Error, Result,
    node::{data::*, filter::Filter},
    ports::*,
    registry::tag,
    runtime::GlobalContext,
    shape::FilterBody,
};
pub use compute::{Definition, Ports, add, definition};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Factor {
    /// Output samples per full-detail pixel of the input.
    #[param(default = 1.0, range = 0.01..=16.0)]
    pub factor: f64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct LongEdge {
    /// Output samples along the input's longer side.
    #[param(default = 2048, range = 1..=65535)]
    pub pixels: i64,
}
#[derive(Clone, crate::Choice)]
pub enum Size {
    #[default]
    #[choice("factor")]
    #[label("Factor")]
    Factor(Factor),
    #[choice("long_edge")]
    #[label("Long edge")]
    LongEdge(LongEdge),
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Reconstruction and antialiasing filter, scaled by each sample's footprint.
    #[param]
    pub filter: Filter,
    /// Full-detail output size, relative to the input or by its long edge.
    #[param]
    pub size: Size,
}
fn validate(p: &Settings) -> Result<()> {
    let valid = match &p.size {
        Size::Factor(f) => f.factor.is_finite() && (0.01..=16.0).contains(&f.factor),
        Size::LongEdge(l) => (1..=65535).contains(&l.pixels),
    };
    if !valid {
        return Err(Error::Contract("resample size is out of range".into()));
    }
    Ok(())
}

/// Full-detail output extent for an input of `full` pixels.
fn full_extent(p: &Settings, full: &Extent) -> Result<Extent> {
    let [w, h] = [f64::from(full.width), f64::from(full.height)];
    let scale = match &p.size {
        Size::Factor(f) => f.factor,
        Size::LongEdge(l) => l.pixels as f64 / w.max(h),
    };
    let side = |v: f64| (v * scale).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    let extent = Extent { width: side(w), height: side(h) };
    extent.validate()?;
    Ok(extent)
}

fn contract(global: &GlobalContext, p: &Settings, image: &RgbDesc) -> Result<(RgbDesc,)> {
    let full = full_extent(p, &image.full)?;
    let extent = full.reduced(global.scale);
    Ok((RgbDesc { extent, full, window: None, coordinates: image.coordinates.clone() },))
}

/// Reconstruct the image and sample it on a new grid, with one filter for
/// magnification and minification: centred on each output sample's position
/// in the input and widened by the local footprint, so magnifying
/// interpolates and minifying antialiases [1]. Weights are normalized, and
/// taps beyond the edge repeat the edge sample. Warps directly before it,
/// such as Perspective Transform, resample once with this filter. After a
/// tone or colour operation it resamples that operation's output. Lanczos-3
/// (default) is sharp but rings: in scene-linear HDR it leaves signed halos
/// beside highlights, which are kept. Size scales the input's full-detail
/// extent or fits its long edge to a pixel count. Preview detail stores
/// proportionally fewer samples of the same framing. Coordinates are kept.
#[crate::node(id = "resample", name = "Resample", tags = [tag::GEOMETRY], contract = contract, resample, references = [
    ("[1] Heckbert, Fundamentals of texture mapping and image warping", "https://www2.eecs.berkeley.edu/Pubs/TechRpts/1989/5504.html"),
])]
pub fn compute(
    p: &Settings,
    _image: Point<'_, Rgb>,
    _output: Point<'_, Rgb>,
) -> Result<&'static FilterBody> {
    Ok(p.filter.body())
}
