//! Image storage, sensor characterization and semantic color types.
//!
//! Consumers ask for the weakest contract that makes their operation meaningful:
//!
//! | Contract | Promise | Consumer |
//! |----------|---------|----------|
//! | [`ThreeChannelMatrix`] | Three samples per pixel and image geometry | Channel access |
//! | [`LinearThreeChannelMatrix`] | Samples scale with represented light | Histogram, waveform |
//! | [`ColorspaceRgbMatrix`] | A declared linear RGB basis with a D65 XYZ transform | Vectorscope |
//! | [`RgbIn<Rec2020>`] | That basis is specifically Rec.2020 | Preview shader |
//!
//! Each row extends the previous one. Scene/display reference is a separate axis:
//! [`SceneRec2020`] and [`DisplayRec2020`] share the same basis, but exposure
//! requires the former and export requires the latter. Sharing a capability
//! does not make the concrete types interchangeable or perform a conversion.
//!
//! Like typeclass laws, the laws below are implementer obligations. Rust checks
//! signatures, not whether pixels are linear or a matrix describes them.
//! Constructors and kernels must preserve these laws; registration asserts them.
//!
//! The base trait deliberately avoids “tristimulus”: camera channels need not
//! determine CIE XYZ uniquely. An exact linear camera-to-XYZ relation requires
//! the Luther condition on spectral sensitivities; typical cameras only
//! approximate it \[8\]. A characterization matrix is an estimate, not proof of
//! that condition. Even known colorimetry does not promise an invertible display
//! mapping after gamut clipping or tone mapping.
//!
//! \[8\] Y. Zhu and G. D. Finlayson, “A Mathematical Investigation into the Design
//! of Prefilters That Make Cameras More Colorimetric,” *Sensors*, 2020,
//! doi: [10.3390/s20236882](https://doi.org/10.3390/s20236882).
//! Full reference and credit are in `THIRD_PARTY.md`.

use crate::color::{self, Mat3};
pub use drip_libraw::Metadata as RawMetadata;
use std::{
    marker::PhantomData,
    sync::{Arc, LazyLock},
};

/// Interleaved 3-channel f32 image, row-major.
/// This buffer alone carries no color interpretation; its owning image does.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    /// Sensor pixels per image pixel along each axis.
    pub scale: u32,
    pub pixels: Vec<[f32; 3]>,
}

impl Rgb {
    /// The same geometry with `f` applied to every pixel.
    pub fn map(&self, f: impl Fn([f32; 3]) -> [f32; 3]) -> Rgb {
        Rgb { pixels: self.pixels.iter().map(|&p| f(p)).collect(), ..*self }
    }
}

/// One color sample per site, normalized so that black is 0 and sensor
/// saturation is 1. Values outside [0, 1] are not clipped.
#[derive(Debug, Clone, PartialEq)]
pub struct Mosaic {
    pub width: usize,
    pub height: usize,
    pub scale: u32,
    pub cfa: Cfa,
    /// Conservative saturation per CFA color in the same units as `data`.
    /// White balance scales these with the samples; highlight detection must
    /// not assume a balanced channel still clips at 1.
    pub white: [f32; 4],
    pub data: Vec<f32>,
    pub camera: Arc<Camera>,
}

/// A color filter array repeating every `size` sites in both directions;
/// colors are 0 R, 1 G, 2 B and 3 for a Bayer cell's second G, which some
/// cameras balance separately.
#[derive(Debug, Clone, PartialEq)]
pub struct Cfa {
    pub size: usize,
    pub colors: Vec<u8>,
}

impl Cfa {
    pub fn color(&self, row: usize, col: usize) -> u8 {
        self.colors[row % self.size * self.size + col % self.size]
    }
}

/// How to interpret a camera's RGB.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// CIE XYZ (D65) to camera RGB.
    pub xyz_to_cam: [[f32; 3]; 3],
    /// As-shot white balance multipliers per CFA color, with G = 1.
    pub white_balance: [f32; 4],
}

/// Three-channel samples and their geometry, without an implicit conversion.
/// “Matrix” means the spatial pixel array, not a 3×3 color transform. RGB and
/// LMS-like channels fit this storage contract without claiming CIE colorimetry.
///
/// # Laws
/// `rgb()` borrows the same image on every call: `pixels.len() == width * height`,
/// row-major, with one consistent channel order and unit convention throughout.
/// `scale` is positive and records sensor pixels per image pixel along each axis.
/// Reading through this trait must not perform a conversion or change metadata.
pub trait ThreeChannelMatrix: Send + Sync {
    fn rgb(&self) -> &Arc<Rgb>;
}

/// Samples are linear in the light quantity they represent; encoded RGB is excluded.
/// EV scopes need linear samples but do not need primaries or a white point,
/// so uncalibrated camera channels can participate here.
///
/// # Laws
/// Zero denotes zero signal, without a black offset or nonlinear transfer curve.
/// Scaling represented light by `k` scales each sample by `k`, up to numerical
/// error. This is relative to the image's reference: tone-mapped display RGB is
/// linear in display light, not in the original scene exposure. Negative and
/// above-one values remain permitted; this contract does not assert a gamut.
pub trait LinearThreeChannelMatrix: ThreeChannelMatrix {}

/// A declared RGB interpretation for chromaticity, without promising camera
/// characterization accuracy or displayability.
///
/// # Laws
/// `color_space().to_xyz_d65 * rgb` gives XYZ in the common D65 frame, using the
/// stored channel order and relative units (RGB white `[1, 1, 1]` maps to D65
/// with Y = 1). The matrix is finite, nonsingular and stable for this image.
/// Its columns describe the same primaries used to interpret the samples;
/// chromatic adaptation is included when the source white differs from D65.
pub trait ColorspaceRgbMatrix: LinearThreeChannelMatrix {
    fn color_space(&self) -> &LinearRgbColorSpace;
}

/// A specific RGB basis `C`, for consumers with fixed coefficients such as the
/// preview shader. Scene/display reference remains independent.
///
/// # Laws
/// Pixels and their color-space definition must agree with `C`'s promised basis:
/// `RgbIn<Rec2020>` means Rec.2020/D65. The marker performs no conversion.
pub trait RgbIn<C>: ColorspaceRgbMatrix {}

/// The transform required by [`ColorspaceRgbMatrix`]. Primary markers come from
/// its columns so a separate primary definition cannot drift out of sync.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearRgbColorSpace {
    pub name: &'static str,
    pub to_xyz_d65: Mat3,
}

/// The RGB basis carried with an image. Keeping the definition beside the
/// samples prevents a scope from interpreting them using unrelated metadata.
///
/// # Laws
/// `definition()` is stable for the value and obeys the matrix/white conventions
/// of [`ColorspaceRgbMatrix`]. A fixed-basis type such as [`Rec2020`] must give
/// the same basis for every instance; a runtime-defined space must retain its
/// actual definition with each image rather than rely on its Rust type alone.
pub trait RgbColorSpace: Send + Sync {
    fn definition(&self) -> &LinearRgbColorSpace;
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rec2020;

impl RgbColorSpace for Rec2020 {
    fn definition(&self) -> &LinearRgbColorSpace {
        static SPACE: LazyLock<LinearRgbColorSpace> = LazyLock::new(|| LinearRgbColorSpace {
            name: "Rec.2020",
            to_xyz_d65: color::rgb_to_xyz(color::REC2020, color::D65),
        });
        &SPACE
    }
}

/// Samples describe scene light before the display rendering transform.
/// Relative exposure remains a scene operation; a color-space conversion alone
/// does not change this reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneReferred;
/// Samples describe intended display light after rendering/tone mapping.
/// They are still linear-light values; an output transfer function is applied
/// later for an encoded export. This marker does not assert a [0, 1] bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayReferred;

/// Linear pixels with independent color-space (`C`) and reference (`R`) types.
/// A reference marker prevents scene/display mixups without duplicating storage
/// or scope implementations. Constructing this wrapper asserts its semantics;
/// it does not convert pixels. Processing nodes must do the corresponding work.
// C: color space. R: scene/display reference (also used by the impls below).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearRgbImage<C, R> {
    data: Arc<Rgb>,
    color_space: C,
    reference: PhantomData<R>,
}

impl<C, R> LinearRgbImage<C, R> {
    pub fn new(data: Arc<Rgb>, color_space: C) -> Self {
        Self { data, color_space, reference: PhantomData }
    }
}

impl<C, R> From<Arc<Rgb>> for LinearRgbImage<C, R>
where
    C: Default,
{
    fn from(data: Arc<Rgb>) -> Self {
        Self::new(data, C::default())
    }
}

impl<C, R> ThreeChannelMatrix for LinearRgbImage<C, R>
where
    C: RgbColorSpace,
    R: Send + Sync,
{
    fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}
impl<C, R> LinearThreeChannelMatrix for LinearRgbImage<C, R>
where
    C: RgbColorSpace,
    R: Send + Sync,
{
}
impl<C, R> ColorspaceRgbMatrix for LinearRgbImage<C, R>
where
    C: RgbColorSpace,
    R: Send + Sync,
{
    fn color_space(&self) -> &LinearRgbColorSpace {
        self.color_space.definition()
    }
}
impl<C, R> RgbIn<C> for LinearRgbImage<C, R>
where
    C: RgbColorSpace,
    R: Send + Sync,
{
}

pub type SceneRec2020 = LinearRgbImage<Rec2020, SceneReferred>;
pub type DisplayRec2020 = LinearRgbImage<Rec2020, DisplayReferred>;

/// Exposure-linear camera responses with their characterization attached.
/// Deliberately lacks `ColorspaceRgbMatrix`: camera conversion must establish
/// a defined RGB interpretation before chromaticity or display consumers run.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraRgb {
    pub data: Arc<Rgb>,
    pub camera: Arc<Camera>,
}
impl ThreeChannelMatrix for CameraRgb {
    fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}
impl LinearThreeChannelMatrix for CameraRgb {}
