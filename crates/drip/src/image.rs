//! The value on an edge and where its samples live.
//!
//! A node asks a value for the residency it needs, so signatures name
//! payloads only. A [`Raster`] holds a description and at most one host and
//! one device representation; the missing one is made on first request and
//! counted as a transfer on its runtime. An [`Image`] is the type-erased value
//! that evaluation and the store pass between nodes: a raster, or a deferred
//! image that a map, warp or resample published. `plan` decides how each step
//! composes, and materializes a deferred image into a raster when a consumer
//! first needs samples. A materialized image reports its bytes per side to
//! the store, which may keep it without one side.
use crate::{
    Error, Result,
    payload::{DeviceBuffer, DeviceLayout, Payload},
    plan::{self, Plan},
    ports::typed,
    runtime::{KernelContext, RuntimeContext},
    shape::{FilterBody, MapBody, MapPayload, Warp},
};
use cubecl::bytes::Bytes;
use std::{
    any::Any,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

/// A type-erased payload description, as contracts receive and return it.
pub type Description = Arc<dyn Any + Send + Sync>;

/// The value an output publishes. Clones share it, including a deferred
/// image's materialization.
#[derive(Clone)]
pub struct Image {
    desc: Description,
    value: Arc<Value>,
}
struct Value {
    /// A `Raster<P>`, set when the image is created or materialized.
    raster: OnceLock<Box<dyn Stored>>,
    /// The computation of a deferred image until it is materialized, which
    /// releases its input.
    pending: Mutex<Option<Deferred>>,
}
/// A plan over its input image, a raster or a deferred image the plan cuts
/// at. `materialize` is captured where the payload type is known, like
/// [`Resident`]'s `decode`.
struct Deferred {
    input: Image,
    plan: Plan,
    materialize: Materialize,
}
type Materialize = fn(&Deferred, &Description, &KernelContext<'_>) -> Result<Box<dyn Stored>>;

/// A raster without its payload type, as the store accounts for it.
trait Stored: Any + Send + Sync {
    fn footprint(&self) -> Footprint;
    /// The same samples without the representation on `side`, or `None`
    /// when it is the only one.
    fn without(&self, side: Side) -> Option<Box<dyn Stored>>;
}

/// Bytes a raster holds on each side, zero where it has no representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Footprint {
    pub host: u64,
    pub device: u64,
}
impl Footprint {
    pub fn on(self, side: Side) -> u64 {
        match side {
            Side::Host => self.host,
            Side::Device => self.device,
        }
    }
}
/// Where a representation lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Host,
    Device,
}

impl Image {
    pub fn desc(&self) -> &Description {
        &self.desc
    }

    /// The samples as payload `P`. `None` when the image holds another
    /// payload or is deferred and not yet materialized.
    pub fn downcast<P: Payload>(&self) -> Option<&Raster<P>> {
        let raster: &dyn Any = self.value.raster.get()?.as_ref();
        raster.downcast_ref()
    }

    /// The samples' bytes on each side, `None` while the image is deferred.
    pub(crate) fn footprint(&self) -> Option<Footprint> {
        self.value.raster.get().map(|raster| raster.footprint())
    }

    /// The samples without the representation on `side`, sharing the other
    /// one. `None` while deferred or when `side` holds the only one.
    pub(crate) fn without(&self, side: Side) -> Option<Image> {
        let raster = self.value.raster.get()?.without(side)?;
        let value = Value { raster: OnceLock::from(raster), pending: Mutex::new(None) };
        Some(Image { desc: self.desc.clone(), value: Arc::new(value) })
    }

    /// Run a deferred image's plan into a raster on `ctx`'s runtime, its
    /// input's plan first where the plan cuts. The first call queues the
    /// kernels, and later calls and clones share the result. A raster needs
    /// no work. Afterwards [`downcast`](Self::downcast) returns the samples.
    pub fn materialize(&self, ctx: &KernelContext<'_>) -> Result<()> {
        // The lock is held while the kernel is queued, so concurrent
        // consumers launch it once. A failed launch leaves the plan pending.
        let mut pending = self.value.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(deferred) = pending.as_ref() {
            let raster = (deferred.materialize)(deferred, &self.desc, ctx)?;
            assert!(self.value.raster.set(raster).is_ok(), "a deferred image has no raster");
            *pending = None;
        }
        Ok(())
    }

    /// The samples as payload `P`, materialized on `ctx`'s runtime. Fails
    /// when the image holds another payload.
    pub fn raster<P: Payload>(&self, ctx: &KernelContext<'_>) -> Result<Raster<P>> {
        self.materialize(ctx)?;
        self.downcast().cloned().ok_or_else(|| Error::Contract(format!("image is not {}", P::NAME)))
    }

    /// The image `body` makes from this one with `constants`, described by
    /// `desc`. Fails when `desc` is on another lattice or `constants` is not
    /// `body.constants` long.
    pub fn map<P: MapPayload>(
        &self,
        body: &MapBody,
        constants: &[f32],
        desc: Description,
    ) -> Result<Image> {
        if constants.len() != body.constants {
            return Err(Error::Contract(format!(
                "{} reads {} constants",
                body.name, body.constants
            )));
        }
        if P::lattice(&typed(&self.desc)) != P::lattice(&typed(&desc)) {
            return Err(Error::Contract(format!("a map keeps its input's {} lattice", P::NAME)));
        }
        Ok(self.defer::<P>(desc, |plan| Some(plan.then(body, constants))))
    }

    /// This image through `warp` onto the lattice `desc` states. Fails when
    /// a warp declared as a reindex moves samples off the lattice.
    pub fn warp<P: MapPayload>(&self, warp: &Warp, desc: Description) -> Result<Image> {
        let (from, to) = (P::lattice(&typed(&self.desc)), P::lattice(&typed(&desc)));
        if !warp.maps.as_flattened().as_flattened().iter().all(|v| v.is_finite()) {
            return Err(Error::Contract("warp maps must be finite".into()));
        }
        let reindex = plan::reindexes(warp, from, to)?;
        Ok(self.defer::<P>(desc, |plan| plan.warp(warp, reindex)))
    }

    /// This image resampled with `filter` onto the lattice `desc` states,
    /// which covers the same image.
    pub fn resample<P: MapPayload>(
        &self,
        filter: &'static FilterBody,
        desc: Description,
    ) -> Result<Image> {
        let (from, to) = (P::lattice(&typed(&self.desc)), P::lattice(&typed(&desc)));
        Ok(self.defer::<P>(desc, |plan| plan.resample(filter, from, to)))
    }

    /// A deferred image described by `desc`: this image's plan extended by
    /// `step`, or, when `step` returns `None` or this image is a raster, a new
    /// plan over this image.
    fn defer<P: MapPayload>(
        &self,
        desc: Description,
        step: impl Fn(&Plan) -> Option<Plan>,
    ) -> Image {
        let pending = self.value.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let extended = pending
            .as_ref()
            .and_then(|deferred| Some((deferred.input.clone(), step(&deferred.plan)?)));
        let (input, plan) = extended.unwrap_or_else(|| {
            (self.clone(), step(&Plan::default()).expect("the empty plan takes every step"))
        });
        let deferred = Deferred { input, plan, materialize: Deferred::materialize::<P> };
        let value = Value { raster: OnceLock::new(), pending: Mutex::new(Some(deferred)) };
        Image { desc, value: Arc::new(value) }
    }
}
impl<P: Payload> From<Raster<P>> for Image {
    fn from(raster: Raster<P>) -> Self {
        let desc = raster.description();
        let raster: Box<dyn Stored> = Box::new(raster);
        let value = Value { raster: OnceLock::from(raster), pending: Mutex::new(None) };
        Self { desc, value: Arc::new(value) }
    }
}
impl Deferred {
    fn materialize<P: MapPayload>(
        &self,
        desc: &Description,
        ctx: &KernelContext<'_>,
    ) -> Result<Box<dyn Stored>> {
        let input = self.input.raster::<P>(ctx)?;
        Ok(Box::new(self.plan.run(&input, typed(desc), ctx)?))
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

impl<P: Payload> Clone for Resident<P> {
    fn clone(&self) -> Self {
        Self { buffer: self.buffer.clone(), runtime: self.runtime.clone(), decode: self.decode }
    }
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
    /// without the device copy. Sources hand one to each run, so an upload of
    /// loaded data lives with the store entry that holds it, under its budget.
    pub fn share_host(&self) -> Result<Self> {
        self.host()?;
        Ok(Self::new(self.0.desc.clone(), self.0.host.get().cloned(), None))
    }
}

impl<P: Payload> Stored for Raster<P> {
    fn footprint(&self) -> Footprint {
        let bytes = P::byte_len(&self.0.desc) as u64;
        let held = |present: bool| if present { bytes } else { 0 };
        Footprint {
            host: held(self.0.host.get().is_some()),
            device: held(self.0.device.get().is_some()),
        }
    }

    fn without(&self, side: Side) -> Option<Box<dyn Stored>> {
        let (host, device) = (self.0.host.get(), self.0.device.get());
        let (host, device) = match side {
            Side::Host => (None, Some(device?.clone())),
            Side::Device => (Some(host?.clone()), None),
        };
        Some(Box::new(Self::new(self.0.desc.clone(), host, device)))
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
