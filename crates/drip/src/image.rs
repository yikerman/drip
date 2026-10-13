//! The value on an edge and where its samples live.
//!
//! A node asks a value for the residency it needs, so signatures name
//! payloads only. A [`Raster`] holds a description and at most one host and
//! one device representation; the missing one is made on first request and
//! counted as a transfer on its runtime. An [`Image`] is the type-erased value
//! that evaluation and the store pass between nodes. Every image is a raster
//! until the segment compiler adds deferred images (fused maps, warps); ports
//! and rasters keep their interface then.
use crate::{
    Error, Result,
    payload::{DeviceBuffer, DeviceLayout, Payload},
    runtime::{KernelContext, RuntimeContext},
};
use cubecl::bytes::Bytes;
use std::{
    any::Any,
    sync::{Arc, OnceLock},
};

/// A type-erased payload description, as contracts receive and return it.
pub type Description = Arc<dyn Any + Send + Sync>;

/// The value an output publishes. Clones share it.
#[derive(Clone)]
pub struct Image {
    desc: Description,
    raster: Arc<dyn Any + Send + Sync>,
}
impl Image {
    pub fn desc(&self) -> &Description {
        &self.desc
    }
    /// The samples as payload `P`, or `None` when the image holds another payload.
    pub fn downcast<P: Payload>(&self) -> Option<&Raster<P>> {
        self.raster.downcast_ref()
    }
}
impl<P: Payload> From<Raster<P>> for Image {
    fn from(raster: Raster<P>) -> Self {
        Self { desc: raster.description(), raster: Arc::new(raster) }
    }
}

/// Materialized samples of payload `P`, immutable and shared by clones. At
/// least one representation exists; the other is made once, on first request.
///
/// Only payloads with a device layout have a device representation:
///
/// ```compile_fail
/// use drip::{image::Raster, node::data::CaptureMetadata, runtime::KernelContext};
/// fn device(raster: &Raster<CaptureMetadata>, ctx: &KernelContext<'_>) {
///     let _ = raster.device(ctx);
/// }
/// ```
pub struct Raster<P: Payload>(Arc<Samples<P>>);

struct Samples<P: Payload> {
    desc: Arc<P::Desc>,
    host: OnceLock<Arc<P::Host>>,
    device: OnceLock<Resident<P>>,
}

/// A device representation and the runtime that owns it. `decode` is captured
/// where `P: DeviceLayout` is known, so [`Raster::host`] needs no such bound.
struct Resident<P: Payload> {
    buffer: DeviceBuffer<P>,
    runtime: RuntimeContext,
    decode: fn(&P::Desc, Bytes) -> Result<P::Host>,
}

impl<P: Payload> Clone for Raster<P> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<P: Payload> Raster<P> {
    /// Host samples. Fails when `host` disagrees with `desc` ([`Payload::check`]).
    pub fn from_host(desc: impl Into<Arc<P::Desc>>, host: P::Host) -> Result<Self> {
        let desc = desc.into();
        P::check(&desc, &host)?;
        Ok(Self::new(desc, Some(Arc::new(host)), None))
    }

    fn new(desc: Arc<P::Desc>, host: Option<Arc<P::Host>>, device: Option<Resident<P>>) -> Self {
        let host = host.map_or_else(OnceLock::new, OnceLock::from);
        let device = device.map_or_else(OnceLock::new, OnceLock::from);
        Self(Arc::new(Samples { desc, host, device }))
    }

    pub fn desc(&self) -> &P::Desc {
        &self.0.desc
    }

    /// The description as contracts receive it.
    pub(crate) fn description(&self) -> Description {
        self.0.desc.clone()
    }

    /// Host samples. The first call on a device-only raster downloads them,
    /// waiting for the kernels that write them, and checks the result.
    pub fn host(&self) -> Result<&P::Host> {
        if let Some(host) = self.0.host.get() {
            return Ok(host);
        }
        let resident = self.0.device.get().expect("a raster has a host or device representation");
        let bytes = resident.runtime.download(resident.buffer.handle.clone())?;
        let host = (resident.decode)(&self.0.desc, bytes)?;
        P::check(&self.0.desc, &host)?;
        // Concurrent first reads may both download; the cell keeps one result.
        Ok(self.0.host.get_or_init(|| Arc::new(host)))
    }

    /// A new raster sharing these host samples, downloaded first if needed,
    /// without the device copy. Sources hand one to each evaluation, so an
    /// upload of loaded data lives only as long as that evaluation.
    pub fn share_host(&self) -> Result<Self> {
        self.host()?;
        Ok(Self::new(self.0.desc.clone(), self.0.host.get().cloned(), None))
    }
}

impl<P: DeviceLayout> Raster<P> {
    /// Device samples written, or about to be written, by kernels on `ctx`'s
    /// runtime. Fails when the allocation is not `P::byte_len(desc)` bytes.
    pub fn from_device(
        desc: impl Into<Arc<P::Desc>>,
        buffer: DeviceBuffer<P>,
        ctx: &KernelContext<'_>,
    ) -> Result<Self> {
        let desc = desc.into();
        if buffer.handle.size_in_used() != P::byte_len(&desc) as u64 {
            return Err(Error::Contract(format!("{} extent/storage mismatch", P::NAME)));
        }
        Ok(Self::new(desc, None, Some(Resident::new(buffer, ctx.runtime.clone()))))
    }

    /// Device samples. The first call on a host-only raster uploads them on
    /// `ctx`'s runtime. A raster already on the device stays on the runtime
    /// that made it, which is `ctx`'s within one evaluator.
    pub fn device(&self, ctx: &KernelContext<'_>) -> Result<&DeviceBuffer<P>> {
        if let Some(resident) = self.0.device.get() {
            return Ok(&resident.buffer);
        }
        let host = self.0.host.get().expect("a raster has a host or device representation");
        // Encoding retains the host allocation; the upload copies nothing on the host.
        let handle = ctx.runtime.upload(P::encode(&self.0.desc, host)?)?;
        let resident = Resident::new(DeviceBuffer::new(handle), ctx.runtime.clone());
        Ok(&self.0.device.get_or_init(|| resident).buffer)
    }

    /// The same samples read as payload `Q` under `desc`, sharing every
    /// representation that exists. Fails when the byte lengths differ; the
    /// element layout is the caller's claim. Forwarded outputs use this.
    pub fn share_as<Q: DeviceLayout>(&self, desc: impl Into<Arc<Q::Desc>>) -> Result<Raster<Q>> {
        let desc = desc.into();
        if P::byte_len(&self.0.desc) != Q::byte_len(&desc) {
            return Err(Error::Contract(format!("{} extent/storage mismatch", Q::NAME)));
        }
        // Buffer payloads encode and decode without copying.
        let reinterpret = |host: &Arc<P::Host>| -> Result<_> {
            let host = Q::decode(&desc, P::encode(&self.0.desc, host)?)?;
            Q::check(&desc, &host)?;
            Ok(Arc::new(host))
        };
        let host = self.0.host.get().map(reinterpret).transpose()?;
        let device = self.0.device.get().map(|resident| {
            Resident::new(
                DeviceBuffer::new(resident.buffer.handle.clone()),
                resident.runtime.clone(),
            )
        });
        Ok(Raster::new(desc, host, device))
    }
}

impl<P: DeviceLayout> Resident<P> {
    fn new(buffer: DeviceBuffer<P>, runtime: RuntimeContext) -> Self {
        Self { buffer, runtime, decode: P::decode }
    }
}
