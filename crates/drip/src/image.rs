//! Concrete image contracts over shared storage. Working RGB is linear-light
//! Rec.2020/D65 by convention, including after creative nonlinear processing.
//! This describes the result, not preservation of original scene relationships.

pub use drip_libraw::Metadata as RawMetadata;
use std::sync::Arc;

/// Row-major, interleaved real samples. No encoding or color meaning is implied.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMat<const C: usize> {
    pub width: usize,
    pub height: usize,
    /// Sensor pixels per sample along each axis.
    pub scale: u32,
    pub pixels: Vec<[f32; C]>,
}
impl<const C: usize> RawMat<C> {
    pub fn map(&self, f: impl Fn([f32; C]) -> [f32; C]) -> Self {
        Self { pixels: self.pixels.iter().copied().map(f).collect(), ..*self }
    }
}
pub type Rgb = RawMat<3>;

/// Metadata or a marker defining the matrix's concrete interpretation.
/// No runtime trait discovery or implication between interpretations is needed.
pub trait Interpretation: std::fmt::Debug + Send + Sync + 'static {
    const NAME: &'static str;
}
impl<const C: usize, I: Interpretation> crate::value::EdgeValue for RealMat<C, I> {
    const NAME: &'static str = I::NAME;
}

/// Working RGB coordinates, always interpreted as linear Rec.2020/D65 light.
/// Creative kernels may change them arbitrarily; subsequent nodes use the same
/// convention. No bounds, scene fidelity or noise-model applicability is implied.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rec2020;
impl Interpretation for Rec2020 {
    const NAME: &'static str = "Rec.2020 RGB";
}

/// Typed logical value. The same buffer can represent incompatible types;
/// constructing this value declares its interpretation, it does not convert it.
#[derive(Debug, Clone, PartialEq)]
pub struct RealMat<const C: usize, I> {
    pub(crate) data: Arc<RawMat<C>>,
    pub(crate) interpretation: Arc<I>,
}
impl<const C: usize, I> RealMat<C, I> {
    pub fn new(data: Arc<RawMat<C>>, interpretation: I) -> Self {
        Self { data, interpretation: Arc::new(interpretation) }
    }
    /// Replaces samples and geometry while retaining interpretation metadata.
    /// Callers must keep the retained metadata applicable to the new samples.
    pub fn with_buffer(&self, data: Arc<RawMat<C>>) -> Self {
        Self { data, interpretation: self.interpretation.clone() }
    }
    pub fn buffer(&self) -> &Arc<RawMat<C>> {
        &self.data
    }
    pub fn interpretation(&self) -> &I {
        &self.interpretation
    }
}
impl<I> RealMat<3, I> {
    pub fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}
impl<const C: usize, I: Default> From<Arc<RawMat<C>>> for RealMat<C, I> {
    fn from(data: Arc<RawMat<C>>) -> Self {
        Self::new(data, I::default())
    }
}

pub type Rec2020Mat = RealMat<3, Rec2020>;
pub type CameraRgb = RealMat<3, Camera>;
/// Calibrated sensor signal: black is zero, saturation uses channel-specific
/// thresholds, and CFA phase belongs to the interpretation. Negative values and
/// values above saturation remain representable.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorMosaic {
    pub cfa: Cfa,
    pub white: [f32; 4],
    pub camera: Arc<Camera>,
}
impl Interpretation for SensorMosaic {
    const NAME: &'static str = "Sensor mosaic";
}
pub type Mosaic = RealMat<1, SensorMosaic>;

impl RawMat<1> {
    pub fn from_samples(width: usize, height: usize, scale: u32, data: Vec<f32>) -> Self {
        Self { width, height, scale, pixels: data.into_iter().map(|v| [v]).collect() }
    }
}
impl<I> RealMat<1, I> {
    pub fn samples(&self) -> &[f32] {
        self.data.pixels.as_flattened()
    }
}
impl<const C: usize, I> std::ops::Deref for RealMat<C, I> {
    type Target = RawMat<C>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
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

impl Interpretation for Camera {
    const NAME: &'static str = "Camera RGB";
}

impl crate::value::EdgeValue for RawMetadata {
    const NAME: &'static str = "raw metadata";
}
