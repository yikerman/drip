//! Concrete image and calibration payloads shared by processing nodes.
pub use crate::payload::{DeviceBuffer, DeviceLayout, HostBuffer, Payload};
use crate::{Error, Result, node::color, payload::decode_array, ports::Requirement};
use cubecl::bytes::Bytes;
use std::{fmt, sync::Arc};

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

/// Camera identity from the decoder. Responses of the same make and model
/// share coordinates, independently of the individual capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Camera {
    pub make: String,
    pub model: String,
}
impl Camera {
    pub fn of(metadata: &drip_raw::Metadata) -> Self {
        Self { make: metadata.make.clone(), model: metadata.model.clone() }
    }
}

/// Black-relative camera responses, normalized by the sensor white reference.
/// `balance` holds the gains applied per LibRaw color index (R, G, B, second
/// G), relative to green; `None` means unbalanced responses.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub camera: Camera,
    pub balance: Option<[f32; 4]>,
}

/// A linear RGB space by its CIE 1931 xy primaries and white. Identity is
/// exact: only values copied from the same constants are the same space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbSpace {
    pub primaries: [[f64; 2]; 3],
    pub white: [f64; 2],
}
pub const REC2020_D65: RgbSpace = RgbSpace { primaries: color::REC2020, white: color::D65 };

/// What three-channel samples mean: camera responses or a defined RGB space.
/// Samples are linear; there is no encoded RGB producer yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Coordinates {
    Camera(Response),
    Space(RgbSpace),
}
impl fmt::Display for Coordinates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Camera(_) => f.write_str(<CameraRgb as Requirement<Rgb>>::LABEL),
            Self::Space(space) if *space == REC2020_D65 => {
                f.write_str(<Rec2020 as Requirement<Rgb>>::LABEL)
            }
            Self::Space(_) => f.write_str("RGB"),
        }
    }
}

/// `full` is the extent of the same image at full detail
/// (`GlobalContext::scale` 1); preview requests store fewer samples. Stored
/// pixel `i` covers full-detail plane position `(i + 0.5) · full / extent`
/// along each axis, approximately where a reduction dropped or averaged
/// partial edge blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbDesc {
    pub extent: Extent,
    pub full: Extent,
    pub coordinates: Coordinates,
}
impl RgbDesc {
    /// An image stored at full detail.
    pub fn new(extent: Extent, coordinates: Coordinates) -> Self {
        Self { full: extent.clone(), extent, coordinates }
    }
    /// The camera responses these samples hold, if they are camera RGB.
    pub fn response(&self) -> Result<&Response> {
        match &self.coordinates {
            Coordinates::Camera(response) => Ok(response),
            Coordinates::Space(_) => Err(Error::Contract(format!("got {}", self.coordinates))),
        }
    }
}
/// Packed three-channel f32 samples, row-major.
pub struct Rgb;
impl Payload for Rgb {
    const NAME: &'static str = "RGB";
    type Desc = RgbDesc;
    type Host = HostBuffer<[f32; 3]>;
    fn validate(d: &RgbDesc) -> Result<()> {
        d.extent.validate()?;
        d.full.validate()?;
        if d.extent.width > d.full.width || d.extent.height > d.full.height {
            return Err(Error::Contract("RGB stores more samples than its full detail".into()));
        }
        Ok(())
    }
    fn check(d: &RgbDesc, host: &Self::Host) -> Result<()> {
        if host.len() != d.extent.pixels() {
            return Err(Error::Contract("RGB extent/storage mismatch".into()));
        }
        Ok(())
    }
    fn allocate(d: &RgbDesc) -> Self::Host {
        vec![[0.0; 3]; d.extent.pixels()].into()
    }
}
impl DeviceLayout for Rgb {
    fn byte_len(d: &RgbDesc) -> usize {
        d.extent.pixels() * size_of::<[f32; 3]>()
    }
    fn encode(_: &RgbDesc, host: &Self::Host) -> Result<Bytes> {
        Ok(host.shared_bytes())
    }
    fn decode(_: &RgbDesc, bytes: Bytes) -> Result<Self::Host> {
        HostBuffer::from_bytes(bytes)
    }
}

/// RGB samples in linear Rec.2020/D65 coordinates.
pub struct Rec2020;
impl Requirement<Rgb> for Rec2020 {
    const LABEL: &'static str = "Rec.2020 / D65";
    fn check(d: &RgbDesc) -> Result<()> {
        if d.coordinates != Coordinates::Space(REC2020_D65) {
            return Err(Error::Contract(format!("got {}", d.coordinates)));
        }
        Ok(())
    }
}
/// RGB samples in camera response coordinates.
pub struct CameraRgb;
impl Requirement<Rgb> for CameraRgb {
    const LABEL: &'static str = "Camera RGB";
    fn check(d: &RgbDesc) -> Result<()> {
        d.response().map(drop)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayerPhase {
    Rggb,
    Grbg,
    Gbrg,
    Bggr,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BayerDesc {
    pub extent: Extent,
    pub phase: BayerPhase,
    pub response: Response,
}
/// One scalar sample per sensor site, row-major.
pub struct Bayer;
impl Payload for Bayer {
    const NAME: &'static str = "Bayer";
    type Desc = BayerDesc;
    type Host = HostBuffer<f32>;
    fn validate(d: &BayerDesc) -> Result<()> {
        d.extent.validate()
    }
    fn check(d: &BayerDesc, host: &Self::Host) -> Result<()> {
        if host.len() != d.extent.pixels() {
            return Err(Error::Contract("Bayer extent/storage mismatch".into()));
        }
        Ok(())
    }
    fn allocate(d: &BayerDesc) -> Self::Host {
        vec![0.0; d.extent.pixels()].into()
    }
}
impl DeviceLayout for Bayer {
    fn byte_len(d: &BayerDesc) -> usize {
        d.extent.pixels() * size_of::<f32>()
    }
    fn encode(_: &BayerDesc, host: &Self::Host) -> Result<Bytes> {
        Ok(host.shared_bytes())
    }
    fn decode(_: &BayerDesc, bytes: Bytes) -> Result<Self::Host> {
        HostBuffer::from_bytes(bytes)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BayerLevelsDesc {
    pub phase: BayerPhase,
    pub response: Response,
}
/// Clipping levels of the four local Bayer sites, in their sample coordinates.
pub struct BayerLevels;
impl Payload for BayerLevels {
    const NAME: &'static str = "Bayer levels";
    type Desc = BayerLevelsDesc;
    type Host = [f32; 4];
    fn validate(_: &BayerLevelsDesc) -> Result<()> {
        Ok(())
    }
    fn check(_: &BayerLevelsDesc, _: &[f32; 4]) -> Result<()> {
        Ok(())
    }
    fn allocate(_: &BayerLevelsDesc) -> [f32; 4] {
        [0.0; 4]
    }
}
impl DeviceLayout for BayerLevels {
    fn byte_len(_: &BayerLevelsDesc) -> usize {
        size_of::<[f32; 4]>()
    }
    fn encode(_: &BayerLevelsDesc, host: &[f32; 4]) -> Result<Bytes> {
        Ok(Bytes::from_elems(host.to_vec()))
    }
    fn decode(_: &BayerLevelsDesc, bytes: Bytes) -> Result<[f32; 4]> {
        decode_array(&bytes)
    }
}

/// Decoded capture facts and native calibration. A host-only value: its
/// description and value are one shared allocation, so contracts read the facts.
pub struct CaptureMetadata;
impl Payload for CaptureMetadata {
    const NAME: &'static str = "Capture metadata";
    type Desc = Arc<drip_raw::Metadata>;
    type Host = Arc<drip_raw::Metadata>;
    fn validate(d: &Self::Desc) -> Result<()> {
        if !(1..=8).contains(&d.orientation) {
            return Err(Error::Contract(
                "capture orientation must be an EXIF value from 1 to 8".into(),
            ));
        }
        Ok(())
    }
    fn check(d: &Self::Desc, host: &Self::Host) -> Result<()> {
        if !Arc::ptr_eq(d, host) {
            return Err(Error::Contract("capture metadata differs from its description".into()));
        }
        Ok(())
    }
    fn allocate(d: &Self::Desc) -> Self::Host {
        d.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_never_stores_more_than_its_full_detail() {
        let mut desc =
            RgbDesc::new(Extent { width: 4, height: 3 }, Coordinates::Space(REC2020_D65));
        assert!(Rgb::validate(&desc).is_ok());
        desc.full = Extent { width: 4, height: 2 };
        assert!(Rgb::validate(&desc).is_err());
        desc.full = Extent { width: 0, height: 8 };
        assert!(Rgb::validate(&desc).is_err());
    }
}
