//! Concrete image and calibration payloads shared by processing nodes.
use crate::{
    Error, Result,
    image::Plane,
    node::color,
    payload::{Persist, decode_array, read_json, write_json},
    ports::Requirement,
    shape::{Lattice, MapPayload, Rect},
};
pub use crate::{
    image::Raster,
    payload::{DeviceBuffer, DeviceLayout, HostBuffer, Payload},
};
use cubecl::bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::{self, Read, Write},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}
impl Extent {
    pub fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }
    /// The samples stored of a full-detail extent at `GlobalContext::scale`:
    /// one per started `scale × scale` block.
    pub fn reduced(&self, scale: u32) -> Self {
        Self { width: self.width.div_ceil(scale), height: self.height.div_ceil(scale) }
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub camera: Camera,
    pub balance: Option<[f32; 4]>,
}

/// A linear RGB space by its CIE 1931 xy primaries and white. Identity is
/// exact: only values copied from the same constants are the same space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RgbSpace {
    pub primaries: [[f64; 2]; 3],
    pub white: [f64; 2],
}
pub const REC2020_D65: RgbSpace = RgbSpace { primaries: color::REC2020, white: color::D65 };

/// What three-channel samples mean: camera responses or a defined RGB space.
/// Samples are linear; there is no encoded RGB producer yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// RGB samples on a lattice over the image's plane. `full` is the extent of
/// the same image at full detail (`GlobalContext::scale` 1), and preview
/// requests store fewer samples. A raster holding the whole lattice stores
/// `extent` samples, and pixel `i` covers full-detail plane position
/// `(i + 0.5) · full / extent` along each axis, approximately where a
/// reduction dropped or averaged partial edge blocks. A raster holding part
/// of it, a crop or an export tile, states its `window`, and stored pixel `i`
/// is lattice sample `window.origin + i`. Contracts describe whole images.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RgbDesc {
    /// Samples stored per axis.
    pub extent: Extent,
    pub full: Extent,
    /// Where the stored samples sit on the image's lattice, `None` when they
    /// are all of it.
    #[serde(default)]
    pub window: Option<Window>,
    pub coordinates: Coordinates,
}
/// Part of an image's lattice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// The lattice index of the first stored sample.
    pub origin: [u32; 2],
    /// Samples of the whole image on the lattice.
    pub lattice: Extent,
}
impl RgbDesc {
    /// An image stored at full detail.
    pub fn new(extent: Extent, coordinates: Coordinates) -> Self {
        Self { full: extent.clone(), extent, window: None, coordinates }
    }
    /// The description of the whole image these samples are part of.
    pub fn whole(&self) -> RgbDesc {
        match &self.window {
            Some(window) => {
                RgbDesc { extent: window.lattice.clone(), window: None, ..self.clone() }
            }
            None => self.clone(),
        }
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
    const PLANE: Option<Plane> = Some(Plane::of::<Self>());
    fn validate(d: &RgbDesc) -> Result<()> {
        d.extent.validate()?;
        d.full.validate()?;
        let lattice = d.window.as_ref().map_or(&d.extent, |window| &window.lattice);
        lattice.validate()?;
        if lattice.width > d.full.width || lattice.height > d.full.height {
            return Err(Error::Contract("RGB stores more samples than its full detail".into()));
        }
        if let Some(window) = &d.window {
            let fits = |origin: u32, extent: u32, lattice: u32| {
                u64::from(origin) + u64::from(extent) <= u64::from(lattice)
            };
            let [x, y] = window.origin;
            if !fits(x, d.extent.width, lattice.width) || !fits(y, d.extent.height, lattice.height)
            {
                return Err(Error::Contract("RGB window lies outside its lattice".into()));
            }
            if window.origin == [0, 0] && d.extent == *lattice {
                return Err(Error::Contract("an RGB window of the whole lattice is none".into()));
            }
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
    fn byte_len(d: &RgbDesc) -> usize {
        d.extent.pixels() * size_of::<[f32; 3]>()
    }
}
impl DeviceLayout for Rgb {
    fn encode(_: &RgbDesc, host: &Self::Host) -> Result<Bytes> {
        Ok(host.shared_bytes())
    }
    fn decode(_: &RgbDesc, bytes: Bytes) -> Result<Self::Host> {
        HostBuffer::from_bytes(bytes)
    }
}

impl Persist for Rgb {
    fn write(d: &RgbDesc, host: &Self::Host, out: &mut dyn Write) -> io::Result<()> {
        write_json(d, out)?;
        host.write(out)
    }
    fn read(input: &mut dyn Read) -> io::Result<(RgbDesc, Self::Host)> {
        let d: RgbDesc = read_json(input)?;
        let host = HostBuffer::read(input, d.extent.pixels())?;
        Ok((d, host))
    }
}

impl MapPayload for Rgb {
    fn lattice(d: &RgbDesc) -> Lattice {
        let stored = [d.extent.width, d.extent.height];
        let (origin, extent) = match &d.window {
            Some(window) => (window.origin, [window.lattice.width, window.lattice.height]),
            None => ([0, 0], stored),
        };
        let full = [d.full.width, d.full.height];
        Lattice { extent, full, window: Rect { origin, extent: stored } }
    }
    fn on(d: &RgbDesc, lattice: Lattice) -> RgbDesc {
        let extent = |[width, height]: [u32; 2]| Extent { width, height };
        let window = (!lattice.is_whole())
            .then(|| Window { origin: lattice.window.origin, lattice: extent(lattice.extent) });
        RgbDesc {
            extent: extent(lattice.window.extent),
            full: extent(lattice.full),
            window,
            coordinates: d.coordinates.clone(),
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BayerPhase {
    Rggb,
    Grbg,
    Gbrg,
    Bggr,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    fn byte_len(d: &BayerDesc) -> usize {
        d.extent.pixels() * size_of::<f32>()
    }
}
impl DeviceLayout for Bayer {
    fn encode(_: &BayerDesc, host: &Self::Host) -> Result<Bytes> {
        Ok(host.shared_bytes())
    }
    fn decode(_: &BayerDesc, bytes: Bytes) -> Result<Self::Host> {
        HostBuffer::from_bytes(bytes)
    }
}

impl Persist for Bayer {
    fn write(d: &BayerDesc, host: &Self::Host, out: &mut dyn Write) -> io::Result<()> {
        write_json(d, out)?;
        host.write(out)
    }
    fn read(input: &mut dyn Read) -> io::Result<(BayerDesc, Self::Host)> {
        let d: BayerDesc = read_json(input)?;
        let host = HostBuffer::read(input, d.extent.pixels())?;
        Ok((d, host))
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
    fn byte_len(_: &BayerLevelsDesc) -> usize {
        size_of::<[f32; 4]>()
    }
}
impl DeviceLayout for BayerLevels {
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
    fn byte_len(_: &Self::Desc) -> usize {
        size_of::<drip_raw::Metadata>()
    }
}

// JSON writes a non-finite float as null, which fails to read back, so
// such metadata is computed again rather than stored wrong.
impl Persist for CaptureMetadata {
    fn write(d: &Self::Desc, _: &Self::Host, out: &mut dyn Write) -> io::Result<()> {
        write_json(&**d, out)
    }
    fn read(input: &mut dyn Read) -> io::Result<(Self::Desc, Self::Host)> {
        let facts = Arc::new(read_json::<drip_raw::Metadata>(input)?);
        Ok((facts.clone(), facts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Descriptions compare by value, so a stored one must read back bit for
    /// bit, and a stream cut short must fail.
    #[test]
    fn persisted_payloads_read_back_exactly() {
        let space = RgbSpace {
            primaries: [[0.1 + 0.2, 1.0 / 3.0], [0.64, 0.33], [1e-300, 0.06]],
            white: color::D65,
        };
        let desc = RgbDesc::new(Extent { width: 2, height: 1 }, Coordinates::Space(space));
        let host: HostBuffer<[f32; 3]> =
            vec![[0.1, f32::MIN_POSITIVE, -0.0], [f32::NAN, 1e30, 2.0]].into();
        let mut stream = Vec::new();
        Rgb::write(&desc, &host, &mut stream).unwrap();
        let (read, samples) = Rgb::read(&mut stream.as_slice()).unwrap();
        assert_eq!(read, desc);
        let bits = |host: &HostBuffer<[f32; 3]>| {
            host.as_flattened().iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        };
        assert_eq!(bits(&samples), bits(&host));
        assert!(Rgb::read(&mut &stream[..stream.len() - 1]).is_err());
        let mut facts = drip_raw::Metadata { make: "Fixture".into(), ..Default::default() };
        facts.calibration.as_shot = [2.123_456_7, 1.0, 1.7, 0.0];
        facts.sensor.pixel_aspect = 1.0 + f64::EPSILON;
        let facts = Arc::new(facts);
        let mut stream = Vec::new();
        CaptureMetadata::write(&facts, &facts, &mut stream).unwrap();
        assert_eq!(*CaptureMetadata::read(&mut stream.as_slice()).unwrap().0, *facts);
    }

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

    /// A window lies inside its lattice and is not the whole of it, so equal
    /// samples have one description.
    #[test]
    fn rgb_windows_lie_inside_their_lattice() {
        let whole = RgbDesc::new(Extent { width: 8, height: 6 }, Coordinates::Space(REC2020_D65));
        let lattice = Rgb::lattice(&whole);
        assert!(lattice.is_whole());
        let crop = Rgb::on(&whole, lattice.with(Rect { origin: [2, 1], extent: [5, 4] }));
        assert_eq!(crop.extent, Extent { width: 5, height: 4 });
        assert_eq!(crop.window, Some(Window { origin: [2, 1], lattice: whole.extent.clone() }));
        assert!(Rgb::validate(&crop).is_ok());
        assert_eq!(Rgb::lattice(&crop), lattice.with(Rect { origin: [2, 1], extent: [5, 4] }));
        assert_eq!(Rgb::on(&crop, lattice), whole);
        let outside = RgbDesc { extent: Extent { width: 7, height: 4 }, ..crop.clone() };
        assert!(Rgb::validate(&outside).is_err());
        let all = RgbDesc {
            extent: whole.extent.clone(),
            window: Some(Window { origin: [0, 0], lattice: whole.extent.clone() }),
            ..crop
        };
        assert!(Rgb::validate(&all).is_err());
    }
}
