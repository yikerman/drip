//! Concrete image and calibration payloads shared by processing nodes.
pub use crate::payload::{DeviceBuffer, HostBuffer, Interpretation, Payload};
use crate::{Error, Result};
use cubecl::bytes::Bytes;
use std::marker::PhantomData;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}
impl Extent {
    pub fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }
    pub fn validate(&self) -> Result<()> {
        // Kernels use 32-bit indices, including the packed RGB layout.
        if self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > u32::MAX as u64 / 3
        {
            return Err(Error::Contract("nonempty extent must fit 32-bit RGB indexing".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Camera {
    pub coordinates: String,
    pub scale: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Encoding {
    Identity,
    Srgb,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Color {
    pub space: String,
    pub encoding: Encoding,
    pub scale: String,
}
impl Interpretation for Camera {
    fn validate(&self) -> Result<()> {
        if self.coordinates.is_empty() || self.scale.is_empty() {
            return Err(Error::Contract("camera coordinates and scale must be declared".into()));
        }
        Ok(())
    }
}
impl Interpretation for Color {
    fn validate(&self) -> Result<()> {
        if self.space.is_empty() || self.scale.is_empty() {
            return Err(Error::Contract("RGB coordinates and scale must be declared".into()));
        }
        Ok(())
    }
}
// IDs are declarations, not EXIF labels or approximate coefficient comparisons.
// Canonical color/profile registries belong to the subsequent node migration.

#[derive(Clone, Debug)]
pub struct ImageDesc<I> {
    pub extent: Extent,
    pub interpretation: I,
}
pub struct Rgb<I>(PhantomData<I>);
pub type CameraRgb = Rgb<Camera>;
pub type ColorRgb = Rgb<Color>;

impl<I: Interpretation> Payload for Rgb<I> {
    type Desc = ImageDesc<I>;
    type Data = HostBuffer<[f32; 3]>;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        d.extent.validate()?;
        d.interpretation.validate()
    }
    fn validate_cpu(d: &Self::Desc, data: &Self::Data) -> Result<()> {
        Self::validate_desc(d)?;
        if data.len() != d.extent.pixels() {
            return Err(Error::Contract("RGB extent/storage mismatch".into()));
        }
        Ok(())
    }
    fn allocate_cpu(d: &Self::Desc) -> Self::Data {
        vec![[0.0; 3]; d.extent.pixels()].into()
    }
    fn byte_len(d: &Self::Desc) -> usize {
        d.extent.pixels() * size_of::<[f32; 3]>()
    }
    fn encode(_: &Self::Desc, data: &Self::Data) -> Result<Bytes> {
        Ok(data.shared_bytes())
    }
    fn decode(_: &Self::Desc, bytes: Bytes) -> Result<Self::Data> {
        HostBuffer::from_bytes(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct MatrixDesc<F, T> {
    pub source: F,
    pub target: T,
}
pub struct Matrix3<F, T>(PhantomData<(F, T)>);
impl<F: Interpretation, T: Interpretation> Payload for Matrix3<F, T> {
    type Desc = MatrixDesc<F, T>;
    type Data = [[f32; 3]; 3];
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        d.source.validate()?;
        d.target.validate()
    }
    fn validate_cpu(d: &Self::Desc, _: &Self::Data) -> Result<()> {
        Self::validate_desc(d)
    }
    fn allocate_cpu(_: &Self::Desc) -> Self::Data {
        [[0.0; 3]; 3]
    }
    fn byte_len(_: &Self::Desc) -> usize {
        size_of::<Self::Data>()
    }
    fn encode(_: &Self::Desc, data: &Self::Data) -> Result<Bytes> {
        Ok(Bytes::from_elems(data.to_vec()))
    }
    fn decode(_: &Self::Desc, bytes: Bytes) -> Result<Self::Data> {
        decode_array(&bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayerPhase {
    Rggb,
    Grbg,
    Gbrg,
    Bggr,
}
#[derive(Clone, Debug)]
pub struct BayerDesc {
    pub extent: Extent,
    pub phase: BayerPhase,
    pub interpretation: Camera,
}
pub struct Bayer;
impl Payload for Bayer {
    type Desc = BayerDesc;
    type Data = HostBuffer<f32>;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        d.extent.validate()?;
        d.interpretation.validate()
    }
    fn validate_cpu(d: &Self::Desc, data: &Self::Data) -> Result<()> {
        Self::validate_desc(d)?;
        if data.len() != d.extent.pixels() {
            return Err(Error::Contract("Bayer extent/storage mismatch".into()));
        }
        Ok(())
    }
    fn allocate_cpu(d: &Self::Desc) -> Self::Data {
        vec![0.0; d.extent.pixels()].into()
    }
    fn byte_len(d: &Self::Desc) -> usize {
        d.extent.pixels() * size_of::<f32>()
    }
    fn encode(_: &Self::Desc, data: &Self::Data) -> Result<Bytes> {
        Ok(data.shared_bytes())
    }
    fn decode(_: &Self::Desc, bytes: Bytes) -> Result<Self::Data> {
        HostBuffer::from_bytes(bytes)
    }
}

/// Fixed f32 coefficients with their own declared interpretation and port.
pub struct Coefficients<D, const N: usize>(PhantomData<D>);
impl<D: Interpretation, const N: usize> Payload for Coefficients<D, N> {
    type Desc = D;
    type Data = [f32; N];
    fn validate_desc(d: &D) -> Result<()> {
        if N == 0 || N > u32::MAX as usize / 4 {
            return Err(Error::Contract("invalid coefficient layout".into()));
        }
        d.validate()
    }
    fn validate_cpu(d: &D, _: &Self::Data) -> Result<()> {
        Self::validate_desc(d)
    }
    fn allocate_cpu(_: &D) -> Self::Data {
        [0.; N]
    }
    fn byte_len(_: &D) -> usize {
        size_of::<Self::Data>()
    }
    fn encode(_: &D, data: &Self::Data) -> Result<Bytes> {
        Ok(Bytes::from_elems(data.to_vec()))
    }
    fn decode(_: &D, bytes: Bytes) -> Result<Self::Data> {
        decode_array(&bytes)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BayerGainDesc {
    pub phase: BayerPhase,
    pub source: Camera,
    pub target: Camera,
}
impl Interpretation for BayerGainDesc {
    fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.target.validate()
    }
}
pub type BayerGains = Coefficients<BayerGainDesc, 4>;

mod metadata;
pub use metadata::{CaptureData, CaptureMetadata, MetadataDesc};

#[derive(Clone, Debug, PartialEq)]
pub struct BayerLevelDesc {
    pub phase: BayerPhase,
    pub interpretation: Camera,
}
impl Interpretation for BayerLevelDesc {
    fn validate(&self) -> Result<()> {
        self.interpretation.validate()
    }
}
pub type BayerLevels = Coefficients<BayerLevelDesc, 4>;

fn decode_array<T: bytemuck::Pod, const N: usize>(bytes: &[u8]) -> Result<[T; N]> {
    bytemuck::try_cast_slice::<u8, T>(bytes)
        .map_err(|e| Error::Runtime(format!("invalid coefficient layout: {e}")))?
        .try_into()
        .map_err(|_| Error::Runtime("coefficient storage length mismatch".into()))
}
