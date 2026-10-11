//! One capture value combines decoder metadata and normalized processing calibration.
use super::*;
use rkyv::{Archive, Deserialize, Serialize};

pub struct CaptureMetadata;

#[derive(Clone, Debug, Default, Archive, Serialize, Deserialize)]
pub struct CaptureData {
    pub capture: drip_raw::Metadata,
    /// Calibration for normalized Bayer samples, independent of capture facts.
    pub bayer: Option<BayerCalibration>,
    pub matrix: Option<CameraCalibration>,
}

#[derive(Clone, Debug, Archive, Serialize, Deserialize)]
pub struct BayerCalibration {
    pub phase: BayerPhase,
    pub source: Camera,
    pub levels: [f32; 4],
    pub gains: Option<SiteGains>,
}
#[derive(Clone, Debug, Archive, Serialize, Deserialize)]
pub struct SiteGains {
    pub target: Camera,
    /// Local 2×2 site order, not RGB channel order.
    pub values: [f32; 4],
}
#[derive(Clone, Debug, Archive, Serialize, Deserialize)]
pub struct CameraCalibration {
    pub source: Camera,
    pub target: Color,
    pub values: [[f32; 3]; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct BayerCalibrationDesc {
    pub phase: BayerPhase,
    pub source: Camera,
    pub balanced: Option<Camera>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MetadataDesc {
    pub bytes: usize,
    pub orientation: u8,
    pub bayer: Option<BayerCalibrationDesc>,
    pub matrix: Option<MatrixDesc<Camera, Color>>,
}
impl MetadataDesc {
    pub fn of(value: &CaptureData) -> Self {
        Self {
            bytes: rkyv::to_bytes::<rkyv::rancor::Error>(value)
                .map(|b| b.len())
                .unwrap_or(usize::MAX),
            orientation: value.capture.orientation,
            bayer: value.bayer.as_ref().map(|b| BayerCalibrationDesc {
                phase: b.phase,
                source: b.source.clone(),
                balanced: b.gains.as_ref().map(|g| g.target.clone()),
            }),
            matrix: value
                .matrix
                .as_ref()
                .map(|m| MatrixDesc { source: m.source.clone(), target: m.target.clone() }),
        }
    }
}
impl Payload for CaptureMetadata {
    type Desc = MetadataDesc;
    type Data = CaptureData;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        if !(1..=8).contains(&d.orientation) {
            return Err(Error::Contract(
                "capture orientation must be an EXIF value from 1 to 8".into(),
            ));
        }
        if d.bytes == 0 || d.bytes > u32::MAX as usize {
            return Err(Error::Contract("capture metadata size must fit device indexing".into()));
        }
        if let Some(b) = &d.bayer {
            b.source.validate()?;
            if let Some(target) = &b.balanced {
                target.validate()?;
            }
        }
        if let Some(m) = &d.matrix {
            m.source.validate()?;
            m.target.validate()?;
        }
        Ok(())
    }
    fn validate_cpu(d: &Self::Desc, data: &Self::Data) -> Result<()> {
        Self::validate_desc(d)?;
        if *d != MetadataDesc::of(data) {
            return Err(Error::Contract("capture metadata differs from its description".into()));
        }
        for text in data.capture.text_fields() {
            std::str::from_utf8(text).map_err(|e| Error::Contract(e.to_string()))?;
        }
        Ok(())
    }
    fn allocate_cpu(_: &Self::Desc) -> Self::Data {
        CaptureData::default()
    }
    fn byte_len(d: &Self::Desc) -> usize {
        d.bytes
    }

    // Internal transport only, not project persistence. The derived archive
    // preserves float bits; unaligned storage accepts arbitrary transport buffers.
    fn encode(_: &Self::Desc, data: &Self::Data) -> Result<Bytes> {
        rkyv::to_bytes::<rkyv::rancor::Error>(data)
            .map(|b| Bytes::from_elems(b.to_vec()))
            .map_err(|e| Error::Runtime(e.to_string()))
    }
    fn decode(d: &Self::Desc, bytes: Bytes) -> Result<Self::Data> {
        if bytes.len() != d.bytes {
            return Err(Error::Runtime("capture metadata storage mismatch".into()));
        }
        rkyv::from_bytes::<CaptureData, rkyv::rancor::Error>(&bytes)
            .map_err(|e| Error::Runtime(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_rejects_invalid_utf8_and_description_mismatches() {
        let mut data = CaptureData::default();
        data.capture.make = vec![0xff];
        let desc = MetadataDesc::of(&data);
        assert!(CaptureMetadata::validate_cpu(&desc, &data).is_err());
        data.capture.make = "測試".as_bytes().to_vec();
        assert!(CaptureMetadata::validate_cpu(&desc, &data).is_err());
        assert!(CaptureMetadata::validate_cpu(&MetadataDesc::of(&data), &data).is_ok());
        let mut huge = MetadataDesc::of(&data);
        huge.bytes = usize::MAX;
        assert!(CaptureMetadata::validate_desc(&huge).is_err());
    }

    #[test]
    fn capture_fields_survive_transport_and_orientation_must_match() {
        let mut data = CaptureData::default();
        data.capture.orientation = 6;
        data.capture.make = b"Camera".to_vec();
        data.capture.lens.model = b"Lens".to_vec();
        data.capture.calibration.pattern.values = vec![1, 2, 3, 4];
        data.capture.calibration.pattern.width = 2;
        data.capture.calibration.pattern.height = 2;
        data.capture.calibration.dng_color[1].color_matrix[0][1] = -0.25;
        data.capture.gps = Some(drip_raw::Gps {
            latitude: [12., 34., 56.],
            latitude_ref: b'N',
            ..Default::default()
        });
        let desc = MetadataDesc::of(&data);
        let bytes = CaptureMetadata::encode(&desc, &data).unwrap();
        assert_eq!(bytes.len(), CaptureMetadata::byte_len(&desc));
        assert_eq!(CaptureMetadata::decode(&desc, bytes).unwrap().capture, data.capture);
        data.capture.orientation = 8;
        assert!(CaptureMetadata::validate_cpu(&desc, &data).is_err());
        data.capture.orientation = 0;
        assert!(CaptureMetadata::validate_desc(&MetadataDesc::of(&data)).is_err());
    }

    #[test]
    fn transport_checks_archive_and_preserves_calibration_float_bits() {
        let camera = Camera { coordinates: "camera".into(), scale: "relative".into() };
        let data = CaptureData {
            bayer: Some(BayerCalibration {
                phase: BayerPhase::Gbrg,
                source: camera.clone(),
                levels: [0.5, 1., 1.5, 2.],
                gains: Some(SiteGains { target: camera.clone(), values: [2., 1., 3., 1.] }),
            }),
            matrix: Some(CameraCalibration {
                source: camera,
                target: crate::node::raw::working_color(),
                values: [[f32::from_bits(0x7fc01234), -0., f32::INFINITY]; 3],
            }),
            ..Default::default()
        };
        let desc = MetadataDesc::of(&data);
        let bytes = CaptureMetadata::encode(&desc, &data).unwrap();
        let restored = CaptureMetadata::decode(&desc, bytes).unwrap();
        CaptureMetadata::validate_cpu(&desc, &restored).unwrap();
        let bayer = restored.bayer.unwrap();
        assert_eq!(bayer.levels, data.bayer.as_ref().unwrap().levels);
        assert_eq!(bayer.gains.unwrap().values, data.bayer.unwrap().gains.unwrap().values);
        assert_eq!(
            restored.matrix.unwrap().values.map(|r| r.map(f32::to_bits)),
            data.matrix.unwrap().values.map(|r| r.map(f32::to_bits))
        );
        assert!(
            CaptureMetadata::decode(&desc, Bytes::from_elems(vec![0xffu8; desc.bytes])).is_err()
        );
        assert!(
            CaptureMetadata::decode(&desc, Bytes::from_elems(vec![0u8; desc.bytes - 1])).is_err()
        );
    }
}
