//! Logical payload families and their concrete host/device representations.
//! A payload names its Rust host type and description; `DeviceLayout` adds
//! the byte layout kernels read, and `Persist` the byte stream the store's
//! disk tier keeps. Residency and transfers live in `image`.
use crate::Result;
use cubecl::{bytes::Bytes, prelude::BufferArg, server::Handle};
use std::{
    fmt::Debug,
    io::{self, Read, Write},
    marker::PhantomData,
};
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
    /// Bytes one value under `desc` occupies, as the store charges its
    /// budgets. A [`DeviceLayout`] payload states its layout's exact length,
    /// a host-only payload an estimate.
    fn byte_len(desc: &Self::Desc) -> usize;
}

/// A payload kernels read on the device. Numeric buffers keep their host element
/// layout; structured payloads stay on the host and do not implement this.
/// `encode` produces, and `decode` reads, [`Payload::byte_len`] bytes.
pub trait DeviceLayout: Payload {
    fn encode(desc: &Self::Desc, host: &Self::Host) -> Result<Bytes>;
    fn decode(desc: &Self::Desc, bytes: Bytes) -> Result<Self::Host>;
}

/// A payload the store can keep on disk, for foreign nodes declared
/// `foreign = disk`. `read` returns what `write` was given, bit for bit, or
/// fails. It fails on a stream `write` did not produce or that ends early.
/// The store validates what `read` returns before using it.
pub trait Persist: Payload {
    fn write(desc: &Self::Desc, host: &Self::Host, out: &mut dyn Write) -> io::Result<()>;
    fn read(input: &mut dyn Read) -> io::Result<(Self::Desc, Self::Host)>;
}

/// Write `value` as JSON after its byte length (u64, little endian). Finite
/// floats read back bit for bit: serde_json's `float_roundtrip` parses them
/// exactly, which descriptions compared by value (`RgbSpace`) need.
pub(crate) fn write_json(value: &impl serde::Serialize, out: &mut dyn Write) -> io::Result<()> {
    let json = serde_json::to_vec(value)?;
    write_bytes(&json, out)
}
/// Read what [`write_json`] wrote.
pub(crate) fn read_json<T: serde::de::DeserializeOwned>(input: &mut dyn Read) -> io::Result<T> {
    // Descriptions are small; a longer prefix belongs to a corrupt stream.
    let json = read_bytes(input, 1 << 24)?;
    Ok(serde_json::from_slice(&json)?)
}
fn write_bytes(bytes: &[u8], out: &mut dyn Write) -> io::Result<()> {
    out.write_all(&(bytes.len() as u64).to_le_bytes())?;
    out.write_all(bytes)
}
fn read_bytes(input: &mut dyn Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut len = [0; 8];
    input.read_exact(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "length exceeds its limit"));
    }
    let mut bytes = vec![0; len as usize];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}
impl<T: bytemuck::Pod + Send + Sync> HostBuffer<T> {
    /// Write the elements' bytes after their byte length.
    pub(crate) fn write(&self, out: &mut dyn Write) -> io::Result<()> {
        write_bytes(bytemuck::cast_slice(self), out)
    }
    /// Read what [`write`](Self::write) wrote of `len` elements. Fails on any
    /// other length.
    pub(crate) fn read(input: &mut dyn Read, len: usize) -> io::Result<Self> {
        let mut prefix = [0; 8];
        input.read_exact(&mut prefix)?;
        if u64::from_le_bytes(prefix) != (len * size_of::<T>()) as u64 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected sample count"));
        }
        // Read into typed storage, so the buffer keeps the element alignment.
        let mut values = vec![bytemuck::Zeroable::zeroed(); len];
        input.read_exact(bytemuck::cast_slice_mut(&mut values))?;
        Ok(values.into())
    }
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
