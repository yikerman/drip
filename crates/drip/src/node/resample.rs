//! Resample: the explicit materialization of an image on a new lattice,
//! with a chosen filter and size. It has no kernel of its own. Evaluation
//! composes it into the fused resampling stage (`plan.rs`).
use crate::{
    Error, Result,
    body::FilterBody,
    node::{data::*, filter::Filter},
    ports::*,
    registry::tag,
    runtime::GlobalContext,
};
pub use compute::{Definition, Ports, add, definition};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Factor {
    /// Output size as a multiple of the input's.
    #[param(default = 1.0, range = 0.01..=16.0)]
    pub factor: f64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct LongEdge {
    /// Length of the output's longer side, in pixels.
    #[param(default = 2048, range = 1..=65535)]
    pub pixels: i64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Exact {
    /// Output width in pixels.
    #[param(default = 2048, range = 1..=65535)]
    pub width: i64,
    /// Output height in pixels.
    #[param(default = 2048, range = 1..=65535)]
    pub height: i64,
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
    #[choice("exact")]
    #[label("Exact size")]
    Exact(Exact),
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// How pixels are rebuilt. Lanczos-3 is the sharpest and rings most.
    #[param]
    pub filter: Filter,
    /// Scale by a factor, fit the long edge, or set both sides.
    #[param]
    pub size: Size,
}
fn validate(p: &Settings) -> Result<()> {
    let valid = match &p.size {
        Size::Factor(f) => f.factor.is_finite() && (0.01..=16.0).contains(&f.factor),
        Size::LongEdge(l) => (1..=65535).contains(&l.pixels),
        Size::Exact(e) => [e.width, e.height].iter().all(|side| (1..=65535).contains(side)),
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
        // Validation bounds both sides to 1..=65535.
        Size::Exact(e) => return Ok(Extent { width: e.width as u32, height: e.height as u32 }),
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

/// Change the image's size. Size scales by a factor, fits the long edge to a
/// pixel count, or sets width and height, which stretches the image when the
/// aspect ratio changes.
///
/// Filter sets how pixels are rebuilt. Lanczos-3, the default, is the sharpest
/// but leaves light and dark rings beside strong highlights. Mitchell rings
/// less, and Catmull–Rom is a little sharper than Mitchell. Triangle and Box
/// do not ring and are softer, and Box looks blocky when enlarging. Directly
/// after Perspective Transform or Lens correction, it also sets their filter,
/// and the image is still resampled once.
// After Heckbert, the filter is centered on each output sample's position in
// the input and widened by the local footprint, so magnifying interpolates and
// minifying antialiases. Weights are normalized, and taps beyond the edge
// repeat the edge sample. Preview detail stores proportionally fewer samples
// of the same framing. After a tone or color operation it resamples that
// operation's output. Coordinates are kept.
#[crate::node(id = "resample", name = "Resample", tags = [tag::GEOMETRY], contract = contract, resample, references = [
    ("Heckbert, Fundamentals of texture mapping and image warping", "https://www2.eecs.berkeley.edu/Pubs/TechRpts/1989/5504.html"),
])]
pub fn compute(
    p: &Settings,
    _image: Point<'_, Rgb>,
    _output: Point<'_, Rgb>,
) -> Result<&'static FilterBody> {
    Ok(p.filter.body())
}
