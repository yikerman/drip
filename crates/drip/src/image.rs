//! Concrete sample layouts, placements, and payload-specific interpretations.
//!
//! Color coordinates and a relationship to captured light are different claims.
//! Additive RGB coordinates permit mixing the represented colors; they do not
//! establish that those colors still measure the original scene. Refinements
//! are checked on values, not inferred from a Rust marker or graph position.

use crate::compute::{Compute, ComputeError, GpuBuffer};
pub use drip_raw::Metadata as RawMetadata;
use std::{fmt::Debug, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("invalid image: {0}")]
    Invalid(String),
    #[error("image contract not satisfied: {0}")]
    Contract(&'static str),
    #[error(transparent)]
    Compute(#[from] ComputeError),
}

/// Row-major interleaved host samples. `C` is a channel count, not a meaning.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMat<const C: usize> {
    pub width: usize,
    pub height: usize,
    /// Sensor pixels per sample along each axis; zero is invalid.
    pub scale: u32,
    pub pixels: Vec<[f32; C]>,
}
impl<const C: usize> RawMat<C> {
    pub fn validate(&self) -> Result<(), ImageError> {
        if Some(extent::<C>(self.width, self.height, self.scale)?)
            != self.pixels.len().checked_mul(C)
        {
            return Err(ImageError::Invalid("sample count does not match extent".into()));
        }
        Ok(())
    }
    pub fn map(&self, f: impl Fn([f32; C]) -> [f32; C]) -> Self {
        Self { pixels: self.pixels.iter().copied().map(f).collect(), ..*self }
    }
}
pub type Rgb = RawMat<3>;
impl RawMat<1> {
    pub fn from_samples(width: usize, height: usize, scale: u32, data: Vec<f32>) -> Self {
        Self { width, height, scale, pixels: data.into_iter().map(|v| [v]).collect() }
    }
}
fn extent<const C: usize>(width: usize, height: usize, scale: u32) -> Result<usize, ImageError> {
    if C == 0 || width == 0 || height == 0 || scale == 0 {
        return Err(ImageError::Invalid("empty extent, channel count or zero sample scale".into()));
    }
    width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(C))
        .ok_or_else(|| ImageError::Invalid("image extent overflow".into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cpu;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gpu;
mod sealed {
    pub trait Placement {}
}
impl sealed::Placement for Cpu {}
impl sealed::Placement for Gpu {}
/// Storage selection only. The two placements share payload interpretation.
pub trait Placement: sealed::Placement + Clone + Debug + Send + Sync + 'static {
    type Storage<const C: usize>: Clone + Debug + Send + Sync;
    fn extent<const C: usize>(storage: &Self::Storage<C>) -> (usize, usize, u32);
}
impl Placement for Cpu {
    type Storage<const C: usize> = Arc<RawMat<C>>;
    fn extent<const C: usize>(s: &Self::Storage<C>) -> (usize, usize, u32) {
        (s.width, s.height, s.scale)
    }
}
/// Device storage is scalar-packed: RGB is three consecutive `f32` values,
/// never a WGSL `array<vec3<f32>>` with its different array stride.
#[derive(Debug, Clone)]
pub struct DeviceMat<const C: usize> {
    buffer: GpuBuffer,
    width: usize,
    height: usize,
    scale: u32,
}
impl Placement for Gpu {
    type Storage<const C: usize> = DeviceMat<C>;
    fn extent<const C: usize>(s: &Self::Storage<C>) -> (usize, usize, u32) {
        (s.width, s.height, s.scale)
    }
}

/// A family-specific meaning record validates its own structural consistency.
/// This is not a registry of dynamic capabilities or a proof of physical truth.
pub trait Interpretation: Debug + Send + Sync + 'static {
    const NAME: &'static str;
    /// Number of scalar components at each sample site for this payload family.
    const CHANNELS: usize;
    fn validate_channels(&self, channels: usize) -> Result<(), ImageError> {
        if channels != Self::CHANNELS {
            return Err(ImageError::Invalid(format!(
                "{} requires {} channels, got {channels}",
                Self::NAME,
                Self::CHANNELS
            )));
        }
        Ok(())
    }
    fn validate(&self) -> Result<(), ImageError> {
        Ok(())
    }
    fn validate_samples(&self, _samples: &[f32]) -> Result<(), ImageError> {
        Ok(())
    }
}

/// Samples plus an immutable meaning record. Placement does not alter meaning.
#[derive(Debug, Clone)]
pub struct RealMat<const C: usize, I, P: Placement = Cpu> {
    pub(crate) data: P::Storage<C>,
    pub(crate) interpretation: Arc<I>,
}
impl<const C: usize, I, P: Placement> RealMat<C, I, P> {
    pub fn interpretation(&self) -> &I {
        &self.interpretation
    }
    pub fn width(&self) -> usize {
        P::extent(&self.data).0
    }
    pub fn height(&self) -> usize {
        P::extent(&self.data).1
    }
    pub fn scale(&self) -> u32 {
        P::extent(&self.data).2
    }
}
impl<const C: usize, I: Interpretation> RealMat<C, I, Cpu> {
    pub fn try_new(data: Arc<RawMat<C>>, interpretation: I) -> Result<Self, ImageError> {
        data.validate()?;
        interpretation.validate_channels(C)?;
        interpretation.validate()?;
        interpretation.validate_samples(data.pixels.as_flattened())?;
        Ok(Self { data, interpretation: Arc::new(interpretation) })
    }
    /// Construct from internally validated samples. Use `try_new` at untrusted
    /// boundaries so malformed geometry or metadata returns a recoverable error.
    pub fn new(data: Arc<RawMat<C>>, interpretation: I) -> Self {
        Self::try_new(data, interpretation).expect("invalid internal image construction")
    }
    pub fn buffer(&self) -> &Arc<RawMat<C>> {
        &self.data
    }
    pub fn upload(&self, compute: &Compute) -> Result<RealMat<C, I, Gpu>, ImageError> {
        self.data.validate()?;
        self.interpretation.validate_channels(C)?;
        self.interpretation.validate()?;
        self.interpretation.validate_samples(self.data.pixels.as_flattened())?;
        Ok(RealMat {
            data: DeviceMat {
                buffer: compute.upload_f32(self.data.pixels.as_flattened())?,
                width: self.width,
                height: self.height,
                scale: self.scale,
            },
            interpretation: self.interpretation.clone(),
        })
    }
}
impl<const C: usize, I: Interpretation> RealMat<C, I, Gpu> {
    /// Attach a kernel's declared output interpretation to device storage.
    /// The caller establishes data-dependent claims: this checks metadata and
    /// extents without reading samples back merely to repeat a trusted kernel.
    pub fn from_gpu(
        buffer: GpuBuffer,
        width: usize,
        height: usize,
        scale: u32,
        interpretation: I,
    ) -> Result<Self, ImageError> {
        if extent::<C>(width, height, scale)? != buffer.len() {
            return Err(ImageError::Invalid("GPU sample count does not match extent".into()));
        }
        interpretation.validate_channels(C)?;
        interpretation.validate()?;
        Ok(Self {
            data: DeviceMat { buffer, width, height, scale },
            interpretation: Arc::new(interpretation),
        })
    }
    pub fn gpu_buffer(&self) -> &GpuBuffer {
        &self.data.buffer
    }
    pub fn download(&self, compute: &Compute) -> Result<RealMat<C, I, Cpu>, ImageError> {
        let flat = compute.read_f32(&self.data.buffer)?;
        let pixels = flat.as_chunks::<C>().0.to_vec();
        Ok(RealMat {
            data: Arc::new(RawMat {
                width: self.width(),
                height: self.height(),
                scale: self.scale(),
                pixels,
            }),
            interpretation: self.interpretation.clone(),
        })
    }
}
impl<I> RealMat<3, I, Cpu> {
    pub fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}
impl<I> RealMat<1, I, Cpu> {
    pub fn samples(&self) -> &[f32] {
        self.data.pixels.as_flattened()
    }
}
impl<const C: usize, I> std::ops::Deref for RealMat<C, I, Cpu> {
    type Target = RawMat<C>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<const C: usize, I: PartialEq> PartialEq for RealMat<C, I, Cpu> {
    fn eq(&self, rhs: &Self) -> bool {
        self.data == rhs.data && self.interpretation == rhs.interpretation
    }
}
impl<const C: usize, I: Interpretation + Default> From<Arc<RawMat<C>>> for RealMat<C, I, Cpu> {
    fn from(data: Arc<RawMat<C>>) -> Self {
        Self::new(data, I::default())
    }
}

/// Supported coordinate systems, both relative to D65 with reference white Y=1.
/// This enum describes colors represented now, not a measurement of the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorCoordinates {
    LinearRec2020,
    Oklab,
    Unspecified,
}
/// Physical quantity estimated for a referenced scene. This is deliberately
/// narrower than arbitrary RGB values: the claim is about relative CIE XYZ
/// tristimulus values under D65, not spectral reconstruction or sensor counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneQuantity {
    RelativeXyzD65,
}
/// Producer-assigned scene identity and estimation target. Two values refer to
/// the same scene only when their identities agree; equal color spaces or file
/// metadata do not establish correspondence. The producer must keep identities
/// distinct for unrelated captures. This record does not establish registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneReference {
    pub capture: Arc<str>,
    pub quantity: SceneQuantity,
}
/// Optional relationship to captured light. The scale multiplies the referenced
/// relative scene tristimulus estimate. It is not a claim of calibration accuracy,
/// an applicable sensor noise model, or absence of clipped/reconstructed samples.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneRelationship {
    Unspecified,
    SceneLinearEstimate { reference: SceneReference, scale: f32 },
    Rendered,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ColorMeaning {
    pub coordinates: ColorCoordinates,
    pub scene: SceneRelationship,
}
impl Default for ColorMeaning {
    fn default() -> Self {
        Self::rec2020()
    }
}
impl ColorMeaning {
    /// Assign additive Rec.2020/D65 coordinates without inventing scene evidence.
    pub fn rec2020() -> Self {
        Self { coordinates: ColorCoordinates::LinearRec2020, scene: SceneRelationship::Unspecified }
    }
    pub fn require_additive(&self) -> Result<(), ImageError> {
        match self.coordinates {
            ColorCoordinates::LinearRec2020 => Ok(()),
            _ => Err(ImageError::Contract("additive color coordinates required")),
        }
    }
    /// A uniform positive gain preserves proportional response when it was
    /// already asserted. It cannot establish that relationship from unknown data.
    pub fn after_exposure(&self, gain: f32) -> Result<Self, ImageError> {
        self.require_additive()?;
        if !gain.is_finite() || gain <= 0.0 {
            return Err(ImageError::Contract("finite positive exposure gain required"));
        }
        let mut result = self.clone();
        if let SceneRelationship::SceneLinearEstimate { scale, .. } = &mut result.scene {
            *scale *= gain;
        }
        result.validate()?;
        Ok(result)
    }
    /// A rendering curve retains intended color coordinates and drops the
    /// original scene-light relationship; RGB storage does not restore it.
    pub fn after_rendering(&self) -> Self {
        Self { coordinates: self.coordinates, scene: SceneRelationship::Rendered }
    }
}
impl Interpretation for ColorMeaning {
    const NAME: &'static str = "Color image";
    const CHANNELS: usize = 3;
    fn validate(&self) -> Result<(), ImageError> {
        if let SceneRelationship::SceneLinearEstimate { reference, scale } = &self.scene {
            if self.coordinates == ColorCoordinates::Unspecified {
                return Err(ImageError::Contract(
                    "a tristimulus scene estimate requires known color coordinates",
                ));
            }
            if reference.capture.is_empty() {
                return Err(ImageError::Contract("scene estimate requires a capture reference"));
            }
            if !scale.is_finite() || *scale <= 0.0 {
                return Err(ImageError::Contract("finite positive scene scale required"));
            }
        }
        Ok(())
    }
}
pub type ColorImage<P = Cpu> = RealMat<3, ColorMeaning, P>;
pub type CameraRgb<P = Cpu> = RealMat<3, Camera, P>;
pub type Mosaic<P = Cpu> = RealMat<1, SensorMosaic, P>;

/// Borrowed witness that the current color coordinates permit additive mixing.
/// It establishes no scene provenance or noise-model applicability.
pub struct AdditiveColor<'a, P: Placement>(&'a ColorImage<P>);
impl<P: Placement> ColorImage<P> {
    pub fn require_additive_color(&self) -> Result<AdditiveColor<'_, P>, ImageError> {
        self.interpretation.require_additive()?;
        Ok(AdditiveColor(self))
    }
}
impl<P: Placement> std::ops::Deref for AdditiveColor<'_, P> {
    type Target = ColorImage<P>;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

/// Normalized sensor samples, with zero black and channel-specific saturation.
/// Negative and above-saturation values remain representable. CFA phase is tied
/// to this image's first sample; demosaic must preserve that correspondence.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorMosaic {
    pub cfa: Cfa,
    pub white: [f32; 4],
    pub camera: Arc<Camera>,
}
impl Interpretation for SensorMosaic {
    const NAME: &'static str = "Sensor mosaic";
    const CHANNELS: usize = 1;
    fn validate(&self) -> Result<(), ImageError> {
        self.cfa.validate()?;
        if self.white.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(ImageError::Contract("finite positive sensor saturation required"));
        }
        self.camera.validate()
    }
}
/// Repeating color filter pattern: 0=R, 1=G, 2=B, 3=second green.
#[derive(Debug, Clone, PartialEq)]
pub struct Cfa {
    pub size: usize,
    pub colors: Vec<u8>,
}
impl Cfa {
    pub fn validate(&self) -> Result<(), ImageError> {
        if self.size == 0
            || self.size.checked_mul(self.size) != Some(self.colors.len())
            || self.colors.iter().any(|&c| c > 3)
        {
            return Err(ImageError::Contract("invalid color filter pattern"));
        }
        Ok(())
    }
    pub fn color(&self, row: usize, col: usize) -> u8 {
        self.colors[row % self.size * self.size + col % self.size]
    }
}
/// Camera response coordinates and capture characterization. A finite matrix
/// need not be invertible: decoding/demosaic do not require color conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// CIE XYZ D65 to camera response coordinates, under the source model.
    pub xyz_to_cam: [[f32; 3]; 3],
    /// As-shot gains by CFA channel, normalized to green=1.
    pub white_balance: [f32; 4],
}
impl Interpretation for Camera {
    const NAME: &'static str = "Camera RGB";
    const CHANNELS: usize = 3;
    fn validate(&self) -> Result<(), ImageError> {
        if self.xyz_to_cam.as_flattened().iter().any(|v| !v.is_finite())
            || self.white_balance.iter().any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(ImageError::Contract(
                "finite camera matrix and positive white balance required",
            ));
        }
        Ok(())
    }
}

/// Fractional coverage; unrelated to luminance, gain or confidence despite the
/// identical scalar sample layout. Constructors check the [0,1] data refinement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coverage;
impl Interpretation for Coverage {
    const NAME: &'static str = "Coverage mask";
    const CHANNELS: usize = 1;
    fn validate_samples(&self, samples: &[f32]) -> Result<(), ImageError> {
        if samples.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err(ImageError::Contract("coverage samples must be finite and in [0,1]"));
        }
        Ok(())
    }
}
pub type CoverageMask<P = Cpu> = RealMat<1, Coverage, P>;
impl CoverageMask<Cpu> {
    pub fn coverage(data: Arc<RawMat<1>>) -> Result<Self, ImageError> {
        Self::try_new(data, Coverage)
    }
}

impl<const C: usize, I: Interpretation> crate::value::EdgeValue for RealMat<C, I, Cpu> {
    const NAME: &'static str = I::NAME;
    const RESIDENCE: crate::value::Residence = crate::value::Residence::Cpu;
    fn family() -> std::any::TypeId {
        std::any::TypeId::of::<Self>()
    }
    fn materialize(
        value: &crate::value::Value,
        compute: &Compute,
    ) -> Result<crate::value::Value, String> {
        if value.downcast_ref::<Self>().is_some() {
            return Ok(value.clone());
        }
        let peer =
            value.downcast_ref::<RealMat<C, I, Gpu>>().ok_or("incompatible image payload")?;
        Ok(crate::value::Value::new(Arc::new(peer.download(compute).map_err(|e| e.to_string())?)))
    }
}
impl<const C: usize, I: Interpretation> crate::value::EdgeValue for RealMat<C, I, Gpu> {
    const NAME: &'static str = I::NAME;
    const RESIDENCE: crate::value::Residence = crate::value::Residence::Gpu;
    fn family() -> std::any::TypeId {
        std::any::TypeId::of::<RealMat<C, I, Cpu>>()
    }
    fn materialize(
        value: &crate::value::Value,
        compute: &Compute,
    ) -> Result<crate::value::Value, String> {
        if value.downcast_ref::<Self>().is_some() {
            return Ok(value.clone());
        }
        let peer =
            value.downcast_ref::<RealMat<C, I, Cpu>>().ok_or("incompatible image payload")?;
        Ok(crate::value::Value::new(Arc::new(peer.upload(compute).map_err(|e| e.to_string())?)))
    }
}
impl crate::value::EdgeValue for RawMetadata {
    const NAME: &'static str = "Raw metadata";
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_is_checked_at_construction() {
        let data = Arc::new(Rgb { width: 2, height: 1, scale: 1, pixels: vec![[0.0; 3]] });
        assert!(ColorImage::try_new(data, ColorMeaning::rec2020()).is_err());
        assert!(extent::<3>(usize::MAX, 2, 1).is_err());
        let scalar = Arc::new(RawMat::from_samples(1, 1, 1, vec![0.5]));
        assert!(RealMat::<1, ColorMeaning>::try_new(scalar, ColorMeaning::rec2020()).is_err());
    }
    #[test]
    fn color_coordinates_do_not_imply_scene_measurement() {
        let meaning = ColorMeaning::rec2020();
        assert_eq!(meaning.after_exposure(2.0).unwrap().scene, SceneRelationship::Unspecified);
        let scene = ColorMeaning {
            scene: SceneRelationship::SceneLinearEstimate {
                reference: SceneReference {
                    capture: "test capture".into(),
                    quantity: SceneQuantity::RelativeXyzD65,
                },
                scale: 1.0,
            },
            ..meaning
        };
        assert_eq!(
            scene.after_exposure(2.0).unwrap().scene,
            SceneRelationship::SceneLinearEstimate {
                reference: SceneReference {
                    capture: "test capture".into(),
                    quantity: SceneQuantity::RelativeXyzD65
                },
                scale: 2.0,
            }
        );
        let rendered = scene.after_rendering();
        assert!(rendered.require_additive().is_ok());
        assert_eq!(rendered.scene, SceneRelationship::Rendered);
        assert!(
            ColorMeaning { coordinates: ColorCoordinates::Oklab, ..rendered }
                .require_additive()
                .is_err()
        );
        assert!(
            ColorMeaning { coordinates: ColorCoordinates::Unspecified, ..scene }
                .validate()
                .is_err()
        );
    }
    #[test]
    fn mask_domain_is_not_a_color_contract() {
        let mask = Arc::new(RawMat::from_samples(2, 1, 1, vec![0.5, 1.1]));
        assert!(CoverageMask::coverage(mask).is_err());
    }
    #[test]
    fn gpu_roundtrip_preserves_samples_and_meaning() {
        let compute = Compute::new().expect("hardware or software compute adapter required");
        let image = ColorImage::try_new(
            Arc::new(Rgb {
                width: 2,
                height: 1,
                scale: 4,
                pixels: vec![[-0.0, f32::from_bits(0x7fc00001), -0.5], [1.0, 2.0, f32::INFINITY]],
            }),
            ColorMeaning {
                coordinates: ColorCoordinates::Oklab,
                scene: SceneRelationship::Rendered,
            },
        )
        .unwrap();
        let resident = image.upload(&compute).unwrap();
        assert!(
            RealMat::<1, ColorMeaning, Gpu>::from_gpu(
                compute.upload_f32(&[0.5]).unwrap(),
                1,
                1,
                1,
                ColorMeaning::rec2020()
            )
            .is_err()
        );
        assert_eq!(resident.interpretation(), image.interpretation());
        let restored = resident.download(&compute).unwrap();
        assert_eq!(restored.interpretation(), image.interpretation());
        assert_eq!((restored.width, restored.height, restored.scale), (2, 1, 4));
        for (a, b) in image.pixels.as_flattened().iter().zip(restored.pixels.as_flattened()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
