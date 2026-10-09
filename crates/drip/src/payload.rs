//! Logical payload families and their concrete host/device representations.
use crate::{Result, runtime::RuntimeContext};
use cubecl::{prelude::BufferArg, server::Handle};
use std::{fmt::Debug, marker::PhantomData};

pub trait Payload: Send + Sync + 'static {
    type Desc: Clone + Debug + Send + Sync + 'static;
    type Cpu: Send + Sync + 'static;
    type Device: Send + Sync + 'static;
    fn validate_desc(desc: &Self::Desc) -> Result<()>;
    fn validate_cpu(desc: &Self::Desc, data: &Self::Cpu) -> Result<()>;
    fn allocate_cpu(desc: &Self::Desc) -> Self::Cpu;
    fn allocate_device(desc: &Self::Desc, runtime: &RuntimeContext) -> Result<Self::Device>;
    fn upload(
        desc: &Self::Desc,
        data: &Self::Cpu,
        runtime: &RuntimeContext,
    ) -> Result<Self::Device>;
    fn download(
        desc: &Self::Desc,
        data: &Self::Device,
        runtime: &RuntimeContext,
    ) -> Result<Self::Cpu>;
}

/// A typed, contiguous f32 allocation. Construction stays in payload adapters.
/// Payload implementations can also declare structured storage.
pub struct F32Buffer<P> {
    pub(crate) handle: Handle,
    marker: PhantomData<P>,
}
impl<P> F32Buffer<P> {
    pub(crate) fn new(handle: Handle) -> Self {
        Self { handle, marker: PhantomData }
    }
    pub fn argument(&self) -> BufferArg {
        // SAFETY: constructors allocate/upload contiguous f32 data. The scalar
        // vectorization factor is one, matching the kernels' &[f32] arguments.
        unsafe { BufferArg::from_raw_parts(self.handle.clone(), 1) }
    }
}

pub trait Interpretation: Clone + Debug + PartialEq + Send + Sync + 'static {
    fn validate(&self) -> Result<()>;
}
