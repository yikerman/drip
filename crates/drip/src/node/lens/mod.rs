//! Lens correction: distortion, lateral chromatic aberration and vignetting
//! as one radial warp ([`Radial`]). The tables a camera embeds become radial
//! maps in `embedded.rs`, after darktable. This file holds the node, the
//! manual Lensfun models and the fill zoom. Design: `agent-docs/FIELD.md`,
//! "The radial stage".
use crate::{
    Error, Result,
    body::{Radial, Spline, Warp, matrix},
    node::{data::*, filter::Filter},
    ports::*,
    registry::tag,
    runtime::GlobalContext,
};
pub use correct::{Definition, Ports, add, definition};
use drip_raw::Correction;

mod embedded;

/// darktable's fine-tune factors: 1 applies the camera's correction, 0
/// removes it and 2 doubles it.
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Embedded {
    #[param(default = 1.0, range = 0.0..=2.0)]
    #[label("Distortion fine-tune")]
    pub distortion: f64,
    #[param(default = 1.0, range = 0.0..=2.0)]
    #[label("Red CA fine-tune")]
    pub chromatic_red: f64,
    #[param(default = 1.0, range = 0.0..=2.0)]
    #[label("Blue CA fine-tune")]
    pub chromatic_blue: f64,
    #[param(default = 1.0, range = 0.0..=2.0)]
    #[label("Vignetting fine-tune")]
    pub vignetting: f64,
}

/// A Lensfun calibration in Lensfun's units \[1\]: `ptlens` distortion and
/// linear TCA about half the shorter image side, `pa` vignetting about the
/// half diagonal.
///
/// \[1\] Lensfun contributors, “lensfun.h,” `lfDistortionModel`,
/// `lfTCAModel` and `lfVignettingModel`.
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Manual {
    /// Lensfun `ptlens` distortion coefficient.
    // r_d = r_u (a r_u³ + b r_u² + c r_u + 1 − a − b − c), in Hugin units.
    #[param(default = 0.0, range = -0.2..=0.2)]
    #[label("Distortion a")]
    pub a: f64,
    #[param(default = 0.0, range = -0.2..=0.2)]
    #[label("Distortion b")]
    pub b: f64,
    #[param(default = 0.0, range = -0.2..=0.2)]
    #[label("Distortion c")]
    pub c: f64,
    /// Lensfun linear TCA: red's size relative to green.
    // r_d,R = k_R r_d, after distortion.
    #[param(default = 1.0, range = 0.99..=1.01)]
    #[label("TCA red")]
    pub red: f64,
    #[param(default = 1.0, range = 0.99..=1.01)]
    #[label("TCA blue")]
    pub blue: f64,
    /// Lensfun `pa` vignetting coefficient.
    // C_d = C_s / (1 + k₁ r² + k₂ r⁴ + k₃ r⁶), r = 1 at the corner.
    #[param(default = 0.0, range = -1.0..=1.0)]
    #[label("Vignetting k1")]
    pub k1: f64,
    #[param(default = 0.0, range = -1.0..=1.0)]
    #[label("Vignetting k2")]
    pub k2: f64,
    #[param(default = 0.0, range = -1.0..=1.0)]
    #[label("Vignetting k3")]
    pub k3: f64,
}

#[derive(Clone, crate::Choice)]
pub enum Source {
    #[default]
    #[choice("embedded")]
    #[label("Embedded")]
    Embedded(Embedded),
    #[choice("manual")]
    #[label("Manual")]
    Manual(Manual),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, crate::Choice)]
pub enum Fit {
    /// Zoom just enough that no empty border shows.
    #[default]
    #[choice("fill")]
    #[label("Fill")]
    Fill,
    /// Keep the scale. Exposed edges are black.
    #[choice("keep")]
    #[label("Keep")]
    Keep,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The correction the camera recorded, or a Lensfun calibration.
    #[param]
    pub source: Source,
    #[param(default = true)]
    pub distortion: bool,
    #[param(default = true)]
    #[label("Chromatic aberration")]
    pub chromatic_aberration: bool,
    #[param(default = true)]
    pub vignetting: bool,
    /// Zoom to hide exposed borders, or keep the scale.
    #[param]
    pub fit: Fit,
}

/// How strongly each correction applies: a fine-tune factor, zero where its
/// switch is off. Every model reduces to the identity at zero.
struct Strength {
    distortion: f64,
    red: f64,
    blue: f64,
    vignetting: f64,
}

impl Settings {
    fn strength(&self) -> Strength {
        let tune = match &self.source {
            Source::Embedded(e) => [e.distortion, e.chromatic_red, e.chromatic_blue, e.vignetting],
            Source::Manual(_) => [1.0; 4],
        };
        let on = |switch: bool, factor: f64| if switch { factor } else { 0.0 };
        Strength {
            distortion: on(self.distortion, tune[0]),
            red: on(self.chromatic_aberration, tune[1]),
            blue: on(self.chromatic_aberration, tune[2]),
            vignetting: on(self.vignetting, tune[3]),
        }
    }
}

fn contract(
    _: &GlobalContext,
    _: &Settings,
    image: &RgbDesc,
    _: Option<&std::sync::Arc<drip_raw::Metadata>>,
) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Correct lens distortion, color fringes toward the edges (lateral chromatic
/// aberration) and dark corners (vignetting). Place it right after demosaic,
/// before Orientation and before any crop or perspective change.
///
/// Embedded uses the correction the camera recorded in Sony, Fujifilm, DNG,
/// Olympus, OM System and Panasonic RAW files, interpreted as darktable does.
/// Fine-tune scales each part: 0 removes it and 2 doubles it. Files without a
/// recorded correction pass through unchanged. Manual takes a Lensfun
/// calibration: ptlens distortion a, b and c, linear TCA red and blue, and pa
/// vignetting k1, k2 and k3. Use a profile for the same focal length and
/// camera format, on an uncropped image.
///
/// The switches turn each correction off. Fill zooms in just enough that no
/// empty border shows, or zooms out to keep more of the frame when it can.
/// Keep leaves the scale alone, so edges the correction exposes are black.
// Each correction is a radial map about the optical center: channel by
// channel, the output at radius ρ reads the input at s_c(ρ)·ρ, times a
// vignetting gain of that source radius. The center is the frame's, or a DNG's
// own in sensor orientation. Manual mode ignores Lensfun's crop factor, real
// focal length and center offsets. The output keeps the input's description.
#[crate::node(id = "lens-correction", name = "Lens correction", tags = [tag::GEOMETRY, tag::RECOVERY], contract = contract, warp, references = [
    ("darktable developers, lens.cc, 2026", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/lens.cc"),
    ("F. Witherden, Embedded lens correction, darktable PR 7092", "https://github.com/darktable-org/darktable/pull/7092"),
    ("Lensfun contributors, Lens corrections, Lensfun Manual", "https://lensfun.github.io/manual/latest/corrections.html"),
])]
pub fn correct(
    p: &Settings,
    image: Point<'_, Rgb>,
    metadata: Option<Read<'_, CaptureMetadata>>,
    _output: Point<'_, Rgb>,
) -> Result<Warp> {
    let metadata = metadata.map(|m| m.data.host()).transpose()?;
    warp(p, plane(image.desc), metadata.and_then(|m| m.lens.correction.as_ref()))
}

/// The input plane's size: the image's full-detail extent.
pub fn plane(image: &RgbDesc) -> [f64; 2] {
    [f64::from(image.full.width), f64::from(image.full.height)]
}

/// The node's warp for an input plane of `size` and the embedded
/// `correction`: an identity reindex when nothing changes, so the node
/// launches nothing, else the radial part after the fit's zoom.
pub fn warp(p: &Settings, size: [f64; 2], correction: Option<&Correction>) -> Result<Warp> {
    let strength = p.strength();
    let radial = match (&p.source, correction) {
        (Source::Manual(m), _) => manual(m, &strength, size)?,
        (Source::Embedded(_), Some(c)) => embedded::radial(c, &strength, size)?,
        (Source::Embedded(_), None) => return Ok(Warp::reindex(matrix::IDENTITY)),
    };
    let ones = |s: &Spline| s.values().iter().all(|&v| v == 1.0);
    let radial = Radial { gain: radial.gain.filter(|g| !ones(g)), ..radial };
    if radial.ratios.iter().all(ones) && radial.gain.is_none() {
        return Ok(Warp::reindex(matrix::IDENTITY));
    }
    let zoom = match p.fit {
        Fit::Fill => fill(&radial, size)?,
        Fit::Keep => 1.0,
    };
    let ([cx, cy], k) = (radial.centre, 1.0 / zoom);
    let map = [[k, 0.0, cx * (1.0 - k)], [0.0, k, cy * (1.0 - k)], [0.0, 0.0, 1.0]];
    Ok(Warp::radial([map; 3], radial, Filter::Lanczos3.body()))
}

/// Knots of every sampled model, and the radius they reach. A fill zoom of
/// 0.8 reads to `ρ = 1.25` at the corners, and beyond it the last ratio is
/// held. At 7968 × 5320 the fixture lens's Lensfun profile interpolates
/// within 0.002 px of its polynomial, where 64 knots err by 0.014 px.
const KNOTS: usize = Spline::MAX_KNOTS;
const REACH: f64 = 1.25;

/// `f` on [`KNOTS`] uniform knots over `[0, REACH]`.
fn sampled(f: impl Fn(f64) -> f64) -> Result<Spline> {
    let step = REACH / (KNOTS - 1) as f64;
    Spline::new(0.0, step, (0..KNOTS).map(|i| f(i as f64 * step)).collect())
}

/// A frame-centred radial map: `ρ = 1` at the corners.
fn centred(size: [f64; 2], ratios: [Spline; 3], gain: Spline) -> Radial {
    let [w, h] = size.map(|v| v / 2.0);
    Radial { centre: [w, h], radius: w.hypot(h), ratios, gain: Some(gain) }
}

/// The gain that undoes a falloff to `vignette` times the light, bounded
/// as darktable bounds it.
fn undo(vignette: f64) -> f64 {
    1.0 / vignette.max(1e-4)
}

/// [`Manual`]'s models. Each maps the undistorted radius to the distorted
/// one, which is the ratio a correction reads, once `ρ` is converted to
/// Hugin's unit of half the shorter side.
fn manual(m: &Manual, k: &Strength, size: [f64; 2]) -> Result<Radial> {
    let hugin = size[0].hypot(size[1]) / size[0].min(size[1]);
    let green = |rho: f64| {
        let r = hugin * rho;
        1.0 + k.distortion * (m.a * r * r * r + m.b * r * r + m.c * r - m.a - m.b - m.c)
    };
    let ratios = [
        sampled(|rho| green(rho) * (1.0 + k.red * (m.red - 1.0)))?,
        sampled(green)?,
        sampled(|rho| green(rho) * (1.0 + k.blue * (m.blue - 1.0)))?,
    ];
    let gain = sampled(|rho| {
        let r2 = rho * rho;
        undo(1.0 + k.vignetting * r2 * (m.k1 + r2 * (m.k2 + r2 * m.k3)))
    })?;
    Ok(centred(size, ratios, gain))
}

/// The least zoom `z` about the centre at which every channel of every
/// output pixel reads inside the input.
///
/// The output point at distance `t` from the centre reads the input at
/// `s_c(t / (z R)) / z` times that distance in the same direction. The
/// frame holds the centre, so the boundary point stays inside iff
/// `s_c(t / (z R)) ≤ z`. The boundary's distances span `[t_min, t_max]`,
/// from the nearest edge to the farthest corner, so `z` is the fixed point
///
/// ```text
/// z = max_c max { s_c(ρ) : t_min / (z R) ≤ ρ ≤ t_max / (z R) }
/// ```
///
/// Iteration converges while `|s′ρ| < s`, far beyond real lenses.
fn fill(radial: &Radial, [w, h]: [f64; 2]) -> Result<f64> {
    let [cx, cy] = radial.centre;
    let near = cx.min(w - cx).min(cy).min(h - cy);
    let far = cx.max(w - cx).hypot(cy.max(h - cy));
    let bound = |z: f64| {
        let [lo, hi] = [near, far].map(|t| t / (z * radial.radius));
        radial.ratios.iter().map(|s| s.range(lo, hi)[1]).fold(f64::NEG_INFINITY, f64::max)
    };
    let zoom = (0..64).fold(1.0, |z, _| bound(z));
    if !(zoom.is_finite() && zoom > 0.0) {
        return Err(Error::Contract("the lens correction maps the frame onto no image".into()));
    }
    Ok(zoom)
}

#[cfg(test)]
mod tests;
