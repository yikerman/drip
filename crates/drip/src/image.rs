//! Physical storage and logical interpretations are independent. Capabilities
//! classify interpretations; sharing a layout never authorizes a connection.
//! Trait laws are trusted kernel obligations, not proofs inferred from samples.

use crate::color::{self, Mat3};
use bevy_reflect::{GetTypeRegistration, Reflect, TypeRegistration};
pub use drip_libraw::Metadata as RawMetadata;
use std::sync::{Arc, LazyLock};

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

/// An immutable interpretation and the generated dictionary of its capabilities.
/// Published interpretations must not change while cached values reference them.
/// Declaring evidence also checks that the Rust implementations exist:
///
/// ```compile_fail,E0277
/// use bevy_reflect::Reflect;
/// use drip::image::LinearRgb;
/// #[drip::interpretation(LinearRgb)]
/// #[derive(Debug, Reflect)]
/// struct Encoded;
/// ```
pub trait Interpretation: Reflect + GetTypeRegistration + std::fmt::Debug {
    const NAME: &'static str;
    fn expose(registration: &mut TypeRegistration);
    fn dictionary() -> Arc<TypeRegistration> {
        // Only generated, typed construction can populate this cache. Callers
        // receive immutable dictionaries; there is no registration API.
        use std::{
            any::TypeId,
            collections::HashMap,
            sync::{OnceLock, RwLock},
        };
        static CACHE: OnceLock<RwLock<HashMap<TypeId, Arc<TypeRegistration>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let id = TypeId::of::<Self>();
        if let Some(r) = cache.read().expect("dictionary lock").get(&id) {
            return r.clone();
        }
        let mut r = Self::get_type_registration();
        Self::expose(&mut r);
        cache.write().expect("dictionary lock").entry(id).or_insert_with(|| Arc::new(r)).clone()
    }
}

/// Coordinates represent linear signal/light values. This says nothing about
/// linearity of their producer or preservation of original capture relationships.
#[crate::capability]
pub trait Linearity {}

/// Samples have a declared mapping to XYZ D65. No inverse or capture fidelity is
/// promised. Implementations must use the actual interpretation of these samples.
#[crate::capability]
pub trait Colorimetry {
    fn to_xyz_d65(&self, sample: [f32; 3]) -> [f32; 3];
}

/// RGB coordinates intended as linear, colorimetric light. The matrix agrees
/// with to_xyz_d65, is finite/nonsingular, and maps RGB white to D65 at Y=1.
#[crate::capability]
pub trait LinearRgb: Linearity + Colorimetry {
    fn color_space(&self) -> &LinearRgbColorSpace;
}

/// The entire interpretation remains valid under positive uniform sample
/// scaling, including every additional guarantee it carries. Fixed range or
/// original-capture promises generally do not satisfy this law. This permits
/// exposure to preserve the concrete type without silently retaining false laws.
#[crate::capability]
pub trait ScaleInvariant: LinearRgb {}

/// Fixed Rec.2020/D65 coefficients, required by the presentation/export kernels.
#[crate::capability]
pub trait Rec2020Rgb: LinearRgb {}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearRgbColorSpace {
    pub name: &'static str,
    pub to_xyz_d65: Mat3,
}

#[crate::interpretation(Rec2020Rgb, ScaleInvariant)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Reflect)]
pub struct Rec2020;
impl Linearity for Rec2020 {}
impl Colorimetry for Rec2020 {
    fn to_xyz_d65(&self, sample: [f32; 3]) -> [f32; 3] {
        color::apply(&self.color_space().to_xyz_d65, sample.map(f64::from)).map(|v| v as f32)
    }
}
impl LinearRgb for Rec2020 {
    fn color_space(&self) -> &LinearRgbColorSpace {
        static SPACE: LazyLock<LinearRgbColorSpace> = LazyLock::new(|| LinearRgbColorSpace {
            name: "Rec.2020",
            to_xyz_d65: color::rgb_to_xyz(color::REC2020, color::D65),
        });
        &SPACE
    }
}
impl Rec2020Rgb for Rec2020 {}
impl ScaleInvariant for Rec2020 {}

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
    /// Replaces samples and geometry while retaining the interpretation witness.
    /// Callers must preserve every law carried by that interpretation.
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
#[crate::interpretation(Linearity)]
#[derive(Debug, Clone, PartialEq, Reflect)]
pub struct SensorMosaic {
    pub cfa: Cfa,
    pub white: [f32; 4],
    pub camera: Arc<Camera>,
}
impl Linearity for SensorMosaic {}
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
#[derive(Debug, Clone, PartialEq, Reflect)]
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
#[crate::interpretation(Linearity)]
#[derive(Debug, Clone, PartialEq, Reflect)]
pub struct Camera {
    /// CIE XYZ (D65) to camera RGB.
    pub xyz_to_cam: [[f32; 3]; 3],
    /// As-shot white balance multipliers per CFA color, with G = 1.
    pub white_balance: [f32; 4],
}

impl Linearity for Camera {}

impl crate::value::EdgeValue for RawMetadata {
    const NAME: &'static str = "raw metadata";
}
