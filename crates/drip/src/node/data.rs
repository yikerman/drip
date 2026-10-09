//! Concrete image and calibration payloads shared by processing nodes.
pub use crate::payload::{F32Buffer, HostBuffer, Interpretation, Payload};
use crate::{Error, Result, runtime::RuntimeContext};
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
        // Kernels use 32-bit indices, including the four-lane RGB layout.
        if self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > u32::MAX as u64 / 4
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
    type Cpu = Vec<[f32; 3]>;
    type Device = F32Buffer<Self>;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        d.extent.validate()?;
        d.interpretation.validate()
    }
    fn validate_cpu(d: &Self::Desc, data: &Self::Cpu) -> Result<()> {
        Self::validate_desc(d)?;
        if data.len() != d.extent.pixels() {
            return Err(Error::Contract("RGB extent/storage mismatch".into()));
        }
        Ok(())
    }
    fn allocate_cpu(d: &Self::Desc) -> Self::Cpu {
        vec![[0.0; 3]; d.extent.pixels()]
    }
    fn allocate_device(d: &Self::Desc, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.empty(d.extent.pixels() * 16)))
    }
    fn upload(_: &Self::Desc, data: &Self::Cpu, r: &RuntimeContext) -> Result<Self::Device> {
        let padded: Vec<[f32; 4]> = data.iter().map(|p| [p[0], p[1], p[2], 0.0]).collect();
        Ok(F32Buffer::new(r.client()?.create(Bytes::from_elems(padded))))
    }
    fn download(_: &Self::Desc, data: &Self::Device, r: &RuntimeContext) -> Result<Self::Cpu> {
        let bytes =
            r.client()?.read_one(data.handle.clone()).map_err(|e| Error::Runtime(e.to_string()))?;
        Ok(bytemuck::cast_slice::<u8, [f32; 4]>(&bytes)
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect())
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
    type Cpu = [[f32; 3]; 3];
    type Device = F32Buffer<Self>;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        d.source.validate()?;
        d.target.validate()
    }
    fn validate_cpu(d: &Self::Desc, _: &Self::Cpu) -> Result<()> {
        Self::validate_desc(d)
    }
    fn allocate_cpu(_: &Self::Desc) -> Self::Cpu {
        [[0.0; 3]; 3]
    }
    fn allocate_device(_: &Self::Desc, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.empty(36)))
    }
    fn upload(_: &Self::Desc, data: &Self::Cpu, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.create(Bytes::from_elems(data.to_vec()))))
    }
    fn download(_: &Self::Desc, data: &Self::Device, r: &RuntimeContext) -> Result<Self::Cpu> {
        let bytes =
            r.client()?.read_one(data.handle.clone()).map_err(|e| Error::Runtime(e.to_string()))?;
        bytemuck::cast_slice::<u8, [f32; 3]>(&bytes)
            .try_into()
            .map_err(|_| Error::Runtime("matrix storage mismatch".into()))
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
    type Cpu = HostBuffer<f32>;
    type Device = F32Buffer<Self>;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        d.extent.validate()?;
        d.interpretation.validate()
    }
    fn validate_cpu(d: &Self::Desc, data: &Self::Cpu) -> Result<()> {
        Self::validate_desc(d)?;
        if data.len() != d.extent.pixels() {
            return Err(Error::Contract("Bayer extent/storage mismatch".into()));
        }
        Ok(())
    }
    fn allocate_cpu(d: &Self::Desc) -> Self::Cpu {
        vec![0.0; d.extent.pixels()].into()
    }
    fn allocate_device(d: &Self::Desc, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.empty(d.extent.pixels() * 4)))
    }
    fn upload(_: &Self::Desc, data: &Self::Cpu, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.create(data.shared_bytes())))
    }
    fn download(_: &Self::Desc, data: &Self::Device, r: &RuntimeContext) -> Result<Self::Cpu> {
        let bytes =
            r.client()?.read_one(data.handle.clone()).map_err(|e| Error::Runtime(e.to_string()))?;
        HostBuffer::from_bytes(bytes)
    }
}

/// Fixed f32 coefficients with their own declared interpretation and port.
pub struct Coefficients<D, const N: usize>(PhantomData<D>);
impl<D: Interpretation, const N: usize> Payload for Coefficients<D, N> {
    type Desc = D;
    type Cpu = [f32; N];
    type Device = F32Buffer<Self>;
    fn validate_desc(d: &D) -> Result<()> {
        if N == 0 || N > u32::MAX as usize / 4 {
            return Err(Error::Contract("invalid coefficient layout".into()));
        }
        d.validate()
    }
    fn validate_cpu(d: &D, _: &Self::Cpu) -> Result<()> {
        Self::validate_desc(d)
    }
    fn allocate_cpu(_: &D) -> Self::Cpu {
        [0.; N]
    }
    fn allocate_device(_: &D, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.empty(N * 4)))
    }
    fn upload(_: &D, data: &Self::Cpu, r: &RuntimeContext) -> Result<Self::Device> {
        Ok(F32Buffer::new(r.client()?.create(Bytes::from_elems(data.to_vec()))))
    }
    fn download(_: &D, data: &Self::Device, r: &RuntimeContext) -> Result<Self::Cpu> {
        let bytes =
            r.client()?.read_one(data.handle.clone()).map_err(|e| Error::Runtime(e.to_string()))?;
        bytemuck::cast_slice::<u8, f32>(&bytes)
            .try_into()
            .map_err(|_| Error::Runtime("coefficient storage mismatch".into()))
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
pub use metadata::{CaptureMetadata, MetadataDesc};

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
