//! Capture metadata is an independent value, never attached to image buffers.
use super::*;

pub struct CaptureMetadata;

pub use drip_raw::Metadata as CaptureData;

#[derive(Clone, Debug)]
pub struct MetadataDesc(pub [usize; 3]);
impl MetadataDesc {
    pub fn of(value: &CaptureData) -> Self {
        Self([value.make.len(), value.model.len(), value.datetime.len()])
    }
    fn bytes(&self) -> usize {
        24 + self.0.iter().sum::<usize>()
    }
}
impl Payload for CaptureMetadata {
    type Desc = MetadataDesc;
    type Data = CaptureData;
    fn validate_desc(d: &Self::Desc) -> Result<()> {
        if d.0
            .iter()
            .try_fold(24usize, |n, &s| n.checked_add(s))
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err(Error::Contract("capture metadata is too large".into()));
        }
        Ok(())
    }
    fn validate_cpu(d: &Self::Desc, data: &Self::Data) -> Result<()> {
        Self::validate_desc(d)?;
        if d.0 != MetadataDesc::of(data).0 {
            return Err(Error::Contract("capture metadata text lengths differ".into()));
        }
        for text in [&data.make, &data.model, &data.datetime] {
            std::str::from_utf8(text).map_err(|e| Error::Contract(e.to_string()))?;
        }
        Ok(())
    }
    fn allocate_cpu(d: &Self::Desc) -> Self::Data {
        CaptureData {
            make: vec![0; d.0[0]],
            model: vec![0; d.0[1]],
            datetime: vec![0; d.0[2]],
            ..Default::default()
        }
    }
    fn byte_len(d: &Self::Desc) -> usize {
        d.bytes()
    }

    // Local adapter for nested vectors: four little-endian f32s, one i64,
    // followed by make/model/datetime bytes. Only contents cross the boundary;
    // vector pointers and capacity never do. The description supplies lengths.
    fn encode(d: &Self::Desc, data: &Self::Data) -> Result<Bytes> {
        let mut bytes = Vec::with_capacity(d.bytes());
        for number in [data.iso, data.shutter, data.aperture, data.focal_length] {
            bytes.extend(number.to_le_bytes());
        }
        bytes.extend(data.timestamp.to_le_bytes());
        for text in [&data.make, &data.model, &data.datetime] {
            bytes.extend(text);
        }
        Ok(Bytes::from_elems(bytes))
    }
    fn decode(d: &Self::Desc, bytes: Bytes) -> Result<Self::Data> {
        if bytes.len() != d.bytes() {
            return Err(Error::Runtime("capture metadata storage mismatch".into()));
        }
        let number = |i| f32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let mut offset = 24;
        let mut text = |len| {
            let value = bytes[offset..offset + len].to_vec();
            offset += len;
            value
        };
        Ok(CaptureData {
            iso: number(0),
            shutter: number(4),
            aperture: number(8),
            focal_length: number(12),
            timestamp: i64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            make: text(d.0[0]),
            model: text(d.0[1]),
            datetime: text(d.0[2]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_rejects_invalid_utf8_and_descriptor_lengths() {
        let mut data = CaptureData { make: vec![0xff], ..Default::default() };
        let desc = MetadataDesc::of(&data);
        assert!(CaptureMetadata::validate_cpu(&desc, &data).is_err());
        data.make = "測試".as_bytes().to_vec();
        assert!(CaptureMetadata::validate_cpu(&desc, &data).is_err());
        assert!(CaptureMetadata::validate_cpu(&MetadataDesc::of(&data), &data).is_ok());
        assert!(CaptureMetadata::validate_desc(&MetadataDesc([usize::MAX, 0, 0])).is_err());
    }
}
