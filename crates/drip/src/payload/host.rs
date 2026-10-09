//! Typed host storage backed by CubeCL's allocation owners.
use crate::{Error, Result};
use bytemuck::Pod;
use cubecl::bytes::{Bytes, Reader, Writer};
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// Contiguous resident host elements. Inputs share storage through the evaluator;
/// freshly allocated outputs have exclusive mutable access.
pub struct HostBuffer<T> {
    bytes: Arc<Bytes>,
    element: PhantomData<T>,
}

impl<T: Pod> HostBuffer<T> {
    /// Retain an already resident allocation, checking its element layout once.
    pub(crate) fn from_bytes(bytes: Bytes) -> Result<Self> {
        if size_of::<T>() == 0 {
            return Err(Error::Runtime("host buffer elements must have nonzero size".into()));
        }
        let slice =
            bytes.read(Reader::new().no_copy()).map_err(|e| Error::Runtime(e.to_string()))?;
        bytemuck::try_cast_slice::<u8, T>(slice)
            .map_err(|e| Error::Runtime(format!("invalid host buffer layout: {e}")))?;
        Ok(Self { bytes: Arc::new(bytes), element: PhantomData })
    }

    /// A queued upload retains this same allocation without copying its bytes.
    pub(crate) fn shared_bytes(&self) -> Bytes {
        let owner = SharedHost(self.bytes.clone());
        Bytes::from_shared(bytes::Bytes::from_owner(owner), self.bytes.property())
    }
}

impl<T: Pod + Send + Sync> From<Vec<T>> for HostBuffer<T> {
    fn from(values: Vec<T>) -> Self {
        Self::from_bytes(Bytes::from_elems(values)).expect("Vec has a valid element layout")
    }
}

impl<T: Pod> Deref for HostBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        bytemuck::cast_slice(
            self.bytes.read(Reader::new().no_copy()).expect("host buffer is resident"),
        )
    }
}

impl<T: Pod> DerefMut for HostBuffer<T> {
    /// Mutable access is for fresh CPU outputs. It never silently copies a
    /// published buffer or storage retained by an outstanding upload.
    fn deref_mut(&mut self) -> &mut [T] {
        let bytes = Arc::get_mut(&mut self.bytes).expect("CPU output must own exclusive storage");
        bytemuck::cast_slice_mut(
            bytes.write(Writer::new().no_copy()).expect("CPU output must own writable storage"),
        )
    }
}

// `from_owner` keeps CubeCL's native/mapped allocation alive through submission.
// This bridge uses the existing shared-bytes controller, not a custom allocator.
struct SharedHost(Arc<Bytes>);
impl AsRef<[u8]> for SharedHost {
    fn as_ref(&self) -> &[u8] {
        self.0.read(Reader::new().no_copy()).expect("host buffer is resident")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_owner_keeps_the_original_allocation_alive() {
        let values = vec![1.0f32, -0.0, f32::from_bits(0x7fc01234)];
        let pointer = values.as_ptr();
        let mut host = HostBuffer::from(values);
        host[0] = 2.0;
        assert_eq!(host.as_ptr(), pointer); // Fresh output writes do not copy.
        let upload = host.shared_bytes();
        let another_upload = host.shared_bytes();
        assert_eq!(upload.as_ptr(), pointer.cast());
        assert_eq!(another_upload.as_ptr(), pointer.cast());
        drop(host);
        drop(upload);
        let values = bytemuck::cast_slice::<u8, f32>(&another_upload);
        assert_eq!(
            values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            [2.0f32.to_bits(), (-0.0f32).to_bits(), 0x7fc01234]
        );
    }

    #[test]
    fn received_bytes_are_retained_and_layout_is_checked() {
        let bytes = Bytes::from_elems(vec![1.0f32, 2.0]);
        let pointer = bytes.as_ptr();
        let host = HostBuffer::<f32>::from_bytes(bytes).unwrap();
        assert_eq!(host.as_ptr().cast::<u8>(), pointer);
        let shared = host.shared_bytes();
        drop(host);
        assert_eq!(bytemuck::cast_slice::<u8, f32>(&shared), [1.0, 2.0]);
        assert!(HostBuffer::<f32>::from_bytes(Bytes::from_elems(vec![0u8; 3])).is_err());
        let bytes = Bytes::from_elems(vec![0u32; 3]).shared();
        assert!(HostBuffer::<f32>::from_bytes(bytes.view(1, 9).unwrap()).is_err());
    }
}
