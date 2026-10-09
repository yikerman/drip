//! Capture metadata is an independent value, never attached to image buffers.
use super::*;
use cubecl::server::Handle;

pub struct CaptureMetadata;
#[derive(Clone, Debug)]
pub struct MetadataDesc(pub [usize; 3]);
impl MetadataDesc {
    pub fn of(value: &drip_raw::Metadata) -> Self {
        Self([value.make.len(), value.model.len(), value.datetime.len()])
    }
    fn bytes(&self) -> usize {
        24 + self.0.iter().sum::<usize>()
    }
}
impl Payload for CaptureMetadata {
    type Desc = MetadataDesc;
    type Cpu = drip_raw::Metadata;
    type Device = Handle;
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
    fn validate_cpu(d: &Self::Desc, data: &Self::Cpu) -> Result<()> {
        Self::validate_desc(d)?;
        if d.0 != MetadataDesc::of(data).0 {
            return Err(Error::Contract("capture metadata string lengths differ".into()));
        }
        Ok(())
    }
    fn allocate_cpu(d: &Self::Desc) -> Self::Cpu {
        drip_raw::Metadata {
            make: "\0".repeat(d.0[0]),
            model: "\0".repeat(d.0[1]),
            datetime: "\0".repeat(d.0[2]),
            ..Default::default()
        }
    }
    fn allocate_device(d: &Self::Desc, runtime: &RuntimeContext) -> Result<Self::Device> {
        Ok(runtime.client()?.empty(d.bytes()))
    }
    fn upload(d: &Self::Desc, data: &Self::Cpu, runtime: &RuntimeContext) -> Result<Self::Device> {
        Self::validate_cpu(d, data)?;
        let mut bytes = Vec::with_capacity(d.bytes());
        for value in [data.iso, data.shutter, data.aperture, data.focal_length] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(data.timestamp.to_le_bytes());
        for text in [&data.make, &data.model, &data.datetime] {
            bytes.extend(text.as_bytes());
        }
        Ok(runtime.client()?.create(Bytes::from_elems(bytes)))
    }
    fn download(
        d: &Self::Desc,
        data: &Self::Device,
        runtime: &RuntimeContext,
    ) -> Result<Self::Cpu> {
        let bytes =
            runtime.client()?.read_one(data.clone()).map_err(|e| Error::Runtime(e.to_string()))?;
        if bytes.len() != d.bytes() {
            return Err(Error::Runtime("capture metadata storage mismatch".into()));
        }
        let number = |i| f32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let mut offset = 24;
        let mut text = |len| -> Result<String> {
            let value = String::from_utf8(bytes[offset..offset + len].to_vec())
                .map_err(|e| Error::Runtime(e.to_string()))?;
            offset += len;
            Ok(value)
        };
        Ok(drip_raw::Metadata {
            iso: number(0),
            shutter: number(4),
            aperture: number(8),
            focal_length: number(12),
            timestamp: i64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            make: text(d.0[0])?,
            model: text(d.0[1])?,
            datetime: text(d.0[2])?,
        })
    }
}
