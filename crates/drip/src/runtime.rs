//! One explicit compute device per evaluator. Host nodes remain ordinary Rust.
//! The runtime counts image transfers and fused launches, which
//! `eval::Statistics` reports, and states the byte budgets of the evaluator's
//! store.
use crate::{
    Error, Result,
    payload::{DeviceBuffer, DeviceLayout},
};
use cubecl::{bytes::Bytes, prelude::*, server::Handle};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering::Relaxed},
};

mod backend;

/// Processing request shared by every node and its contract. A scale of one
/// requests full detail. Demosaic applies this spatial reduction internally;
/// other nodes may preserve their input sampling or define their own policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalContext {
    /// Requested spatial reduction factor. One means full detail.
    pub scale: u32,
}
impl Default for GlobalContext {
    fn default() -> Self {
        Self { scale: 1 }
    }
}
impl GlobalContext {
    pub fn validate(&self) -> Result<()> {
        if !(1..=256).contains(&self.scale) {
            return Err(Error::Contract("global scale must be 1..=256".into()));
        }
        Ok(())
    }
}

/// A compute client, or none for host-only evaluation. Clones share the client
/// and the counts, so a raster keeps the runtime its device buffer belongs to.
#[derive(Clone)]
pub struct RuntimeContext {
    client: Option<Client>,
    counts: Arc<Counts>,
    budget: Budget,
}

/// Bytes an evaluator's store keeps resident on each side before it evicts.
/// Values a running evaluation holds are not bounded by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub device: u64,
    pub host: u64,
}
impl Default for Budget {
    /// 4 GiB on the device and 8 GiB on the host.
    fn default() -> Self {
        Self { device: 4 << 30, host: 8 << 30 }
    }
}
#[derive(Default)]
struct Counts {
    uploads: AtomicUsize,
    downloads: AtomicUsize,
    fused: AtomicUsize,
}

impl RuntimeContext {
    /// Select computation with DRIP_BACKEND, otherwise the platform's included
    /// native backend or wgpu. See the crate README for build features and
    /// compiler/API choices. This setting does not change GUI rendering.
    pub fn from_env() -> Result<Self> {
        let name = match std::env::var("DRIP_BACKEND") {
            Ok(name) => name,
            Err(std::env::VarError::NotPresent) => backend::default_name().into(),
            Err(_) => return Err(Error::Runtime("DRIP_BACKEND must be valid UTF-8".into())),
        };
        Ok(backend::select(&name)?())
    }

    pub fn host() -> Self {
        Self { client: None, counts: Default::default(), budget: Budget::default() }
    }

    /// Adopt an explicitly initialized compute client. Device identity belongs
    /// to this context and its clones. The device budget is half the memory
    /// the runtime reports, or the default where it reports none.
    pub fn from_client(client: Client) -> Self {
        let reported = client.properties().memory.max_memory();
        let device = reported.map_or(Budget::default().device, |bytes| bytes / 2);
        let budget = Budget { device, ..Budget::default() };
        Self { client: Some(client), counts: Default::default(), budget }
    }

    /// This context with the store budgets of evaluators built on it.
    pub fn with_budget(self, budget: Budget) -> Self {
        Self { budget, ..self }
    }

    pub fn budget(&self) -> Budget {
        self.budget
    }

    pub fn client(&self) -> Result<&Client> {
        self.client.as_ref().ok_or_else(|| Error::Runtime("no compute runtime is bound".into()))
    }

    /// Image uploads and downloads on this runtime so far. Constants and
    /// scratch are not counted.
    pub(crate) fn transfers(&self) -> (usize, usize) {
        (self.counts.uploads.load(Relaxed), self.counts.downloads.load(Relaxed))
    }

    /// Fused kernels launched on this runtime so far.
    pub(crate) fn fused(&self) -> usize {
        self.counts.fused.load(Relaxed)
    }

    pub(crate) fn count_fused(&self) {
        self.counts.fused.fetch_add(1, Relaxed);
    }

    pub(crate) fn upload(&self, bytes: Bytes) -> Result<Handle> {
        let handle = self.client()?.create(bytes);
        self.counts.uploads.fetch_add(1, Relaxed);
        Ok(handle)
    }

    /// Waits for the queued work that writes `handle`.
    pub(crate) fn download(&self, handle: Handle) -> Result<Bytes> {
        let bytes = self.client()?.read_one(handle).map_err(|e| Error::Runtime(e.to_string()))?;
        self.counts.downloads.fetch_add(1, Relaxed);
        Ok(bytes)
    }

    pub(crate) fn finish(&self) -> Result<()> {
        if let Some(client) = &self.client {
            pollster::block_on(client.sync()).map_err(|e| Error::Runtime(e.to_string()))?;
        }
        Ok(())
    }
}

pub(crate) fn dispatch_dims(elements: usize) -> (CubeCount, CubeDim) {
    let groups = elements.div_ceil(256);
    let x = groups.min(65535);
    (CubeCount::Static(x as u32, groups.div_ceil(x) as u32, 1), CubeDim::new_1d(256))
}

/// Dispatch and device allocation supplied by evaluation. Nodes use the client
/// to launch kernels; image transfers go through [`crate::image::Raster`], so
/// they are counted.
pub struct KernelContext<'a> {
    pub global: &'a GlobalContext,
    pub(crate) runtime: &'a RuntimeContext,
}
impl<'a> KernelContext<'a> {
    /// For frontends that run kernels or materialize images outside
    /// evaluation, such as a view resampling its input.
    pub fn new(global: &'a GlobalContext, runtime: &'a RuntimeContext) -> Self {
        Self { global, runtime }
    }

    pub fn client(&self) -> Result<&'a Client> {
        self.runtime.client()
    }

    /// Uninitialized device storage for one `P` described by `desc`, for
    /// samples a node computes outside its outputs (a reduced input) or a
    /// frontend computes outside evaluation. Wrap it with
    /// [`Raster::from_device`](crate::image::Raster::from_device) to read it back.
    pub fn device_buffer<P: DeviceLayout>(&self, desc: &P::Desc) -> Result<DeviceBuffer<P>> {
        Ok(DeviceBuffer::new(self.client()?.empty(P::byte_len(desc))))
    }

    /// Per-invocation constants a kernel reads, such as a matrix flattened from
    /// a host payload. Like scratch, they have no port meaning.
    pub fn constants(&self, values: &[f32]) -> Result<DeviceBuffer<()>> {
        let bytes = Bytes::from_elems(values.to_vec());
        Ok(DeviceBuffer::new(self.client()?.create(bytes)))
    }

    /// Algorithm-local scratch for 32-bit scalars (f32 or u32), with no port
    /// meaning. Kernels must initialize each element before reading it.
    pub fn scratch_32(&self, elements: usize) -> Result<DeviceBuffer<()>> {
        let bytes = elements
            .checked_mul(4)
            .filter(|&n| n > 0)
            .ok_or_else(|| Error::Runtime("invalid scratch extent".into()))?;
        Ok(DeviceBuffer::new(self.client()?.empty(bytes)))
    }
}
