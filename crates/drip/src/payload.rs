//! Logical payload families and their concrete host/device representations.
use crate::{Result, runtime::RuntimeContext};
use cubecl::{prelude::BufferArg, server::Handle};
use std::{fmt::Debug, marker::PhantomData};
mod host;
pub use host::HostBuffer;

/// A logical payload with one concrete host layout and an explicit byte codec.
/// Numeric buffers keep the same element layout on the device. Nested allocations
/// use a local packing adapter; allocation and transport remain shared.
pub trait Payload: Send + Sync + Sized + 'static {
    type Desc: Clone + Debug + Send + Sync + 'static;
    type Data: Send + Sync + 'static;
    fn validate_desc(desc: &Self::Desc) -> Result<()>;
    fn validate_cpu(desc: &Self::Desc, data: &Self::Data) -> Result<()>;
    fn allocate_cpu(desc: &Self::Desc) -> Self::Data;
    fn byte_len(desc: &Self::Desc) -> usize;
    fn encode(desc: &Self::Desc, data: &Self::Data) -> Result<cubecl::bytes::Bytes>;
    fn decode(desc: &Self::Desc, bytes: cubecl::bytes::Bytes) -> Result<Self::Data>;

    fn upload(
        desc: &Self::Desc,
        data: &Self::Data,
        runtime: &RuntimeContext,
    ) -> Result<DeviceBuffer<Self>> {
        Self::validate_cpu(desc, data)?;
        Ok(DeviceBuffer::new(runtime.client()?.create(Self::encode(desc, data)?)))
    }
    fn download(
        desc: &Self::Desc,
        data: &DeviceBuffer<Self>,
        runtime: &RuntimeContext,
    ) -> Result<Self::Data> {
        Self::validate_desc(desc)?;
        let bytes = runtime
            .client()?
            .read_one(data.handle.clone())
            .map_err(|e| crate::Error::Runtime(e.to_string()))?;
        let data = Self::decode(desc, bytes)?;
        Self::validate_cpu(desc, &data)?;
        Ok(data)
    }
}

/// A CubeCL allocation tagged with its logical payload (or scratch marker).
pub struct DeviceBuffer<P> {
    pub(crate) handle: Handle,
    marker: PhantomData<P>,
}
impl<P> DeviceBuffer<P> {
    pub(crate) fn new(handle: Handle) -> Self {
        Self { handle, marker: PhantomData }
    }
    pub fn argument(&self) -> BufferArg {
        // SAFETY: payload codecs and scratch allocators determine the byte layout.
        // Scalar vectorization matches the element slices declared by kernels.
        unsafe { BufferArg::from_raw_parts(self.handle.clone(), 1) }
    }
}

pub trait Interpretation: Clone + Debug + PartialEq + Send + Sync + 'static {
    fn validate(&self) -> Result<()>;
}
