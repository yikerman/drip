//! Logical payload families and their concrete host/device representations.
//! A payload names its Rust host type and description; `DeviceLayout` adds
//! the byte layout kernels read. Residency and transfers live in `image`.
use crate::Result;
use cubecl::{bytes::Bytes, prelude::BufferArg, server::Handle};
use std::{fmt::Debug, marker::PhantomData};
mod host;
pub use host::HostBuffer;

/// A logical payload: one concrete host value and a host description that
/// contracts read. Descriptions and values are checked where they enter the
/// graph (producer outputs, loaded sources); internal code trusts them.
pub trait Payload: Send + Sync + Sized + 'static {
    /// Port label shared by help and the editor.
    const NAME: &'static str;
    type Desc: Clone + Debug + Send + Sync + 'static;
    type Host: Send + Sync + 'static;
    fn validate(desc: &Self::Desc) -> Result<()>;
    /// The value agrees with its (valid) description.
    fn check(desc: &Self::Desc, host: &Self::Host) -> Result<()>;
    /// Fresh host output storage for a valid description.
    fn allocate(desc: &Self::Desc) -> Self::Host;
}

/// A payload kernels read on the device. Numeric buffers keep their host element
/// layout; structured payloads stay on the host and do not implement this.
pub trait DeviceLayout: Payload {
    fn byte_len(desc: &Self::Desc) -> usize;
    fn encode(desc: &Self::Desc, host: &Self::Host) -> Result<Bytes>;
    fn decode(desc: &Self::Desc, bytes: Bytes) -> Result<Self::Host>;
}

/// A CubeCL allocation tagged with its logical payload (or `()` for scratch
/// and per-invocation constants). Clones retain the same allocation.
pub struct DeviceBuffer<P> {
    pub(crate) handle: Handle,
    marker: PhantomData<P>,
}
impl<P> Clone for DeviceBuffer<P> {
    fn clone(&self) -> Self {
        Self::new(self.handle.clone())
    }
}
impl<P> DeviceBuffer<P> {
    pub(crate) fn new(handle: Handle) -> Self {
        Self { handle, marker: PhantomData }
    }
    pub fn argument(&self) -> BufferArg {
        // SAFETY: payload layouts and scratch allocators determine the byte layout.
        // Scalar vectorization matches the element slices declared by kernels.
        unsafe { BufferArg::from_raw_parts(self.handle.clone(), 1) }
    }
}

pub(crate) fn decode_array<T: bytemuck::Pod, const N: usize>(bytes: &[u8]) -> Result<[T; N]> {
    bytemuck::try_cast_slice::<u8, T>(bytes)
        .map_err(|e| crate::Error::Runtime(format!("invalid coefficient layout: {e}")))?
        .try_into()
        .map_err(|_| crate::Error::Runtime("coefficient storage length mismatch".into()))
}
