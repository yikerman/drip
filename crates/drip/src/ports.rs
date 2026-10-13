//! Typed views of a node's ports, requirements and port identities.
//!
//! A port names a payload and an interpretation requirement, never a
//! placement. `Read` borrows an input [`Raster`] and the node asks it for
//! `host()` or `device(ctx)`. `Write` creates the output where the node writes
//! it; `Forward` publishes samples the node already holds; a foreign node
//! describes and fills each output through `Publish`. The host function of a
//! map, warp or resample sees its input and output as `Point`, a description
//! without samples. Type erasure is confined to the generated node adapter.
use crate::{
    Error, Result,
    image::{Description, Image, Raster},
    payload::{DeviceBuffer, DeviceLayout, Payload},
    runtime::KernelContext,
};
use std::{any::TypeId, cell::OnceCell, marker::PhantomData, sync::Arc};

/// An interpretation a port requires (inputs) or guarantees (outputs), checked
/// on descriptions before and after the node's contract.
pub trait Requirement<P: Payload>: 'static {
    /// Shown after the payload name; empty when the port accepts any description.
    const LABEL: &'static str;
    fn check(desc: &P::Desc) -> Result<()>;
}
/// No interpretation requirement beyond the payload itself.
pub struct Any;
impl<P: Payload> Requirement<P> for Any {
    const LABEL: &'static str = "";
    fn check(_: &P::Desc) -> Result<()> {
        Ok(())
    }
}

/// An evaluated input. `desc` is `data.desc()`, checked against `R`.
pub struct Read<'a, P: Payload, R = Any> {
    pub desc: &'a P::Desc,
    pub data: &'a Raster<P>,
    requirement: PhantomData<R>,
}
impl<'a, P: Payload, R> Read<'a, P, R> {
    pub fn new(data: &'a Raster<P>) -> Self {
        Self { desc: data.desc(), data, requirement: PhantomData }
    }
    /// View a materialized input ([`Image::materialize`]).
    #[doc(hidden)]
    pub fn bind(image: Option<&'a Image>) -> Option<Self> {
        image.map(|image| Self::new(image.downcast().expect("typed node adapter invariant")))
    }
}

/// The input or the output of a map, warp or resample, as its host function
/// sees them: the description only. The samples exist once a consumer
/// materializes the output. `desc` is checked against `R`.
pub struct Point<'a, P: Payload, R = Any> {
    pub desc: &'a P::Desc,
    requirement: PhantomData<R>,
}
impl<'a, P: Payload, R> Point<'a, P, R> {
    #[doc(hidden)]
    pub fn new(desc: &'a Description) -> Self {
        Self { desc: downcast(desc), requirement: PhantomData }
    }
}

/// An output under construction, described by the contract. The node writes
/// one representation: `host()` or `device(ctx)`, each allocated on first
/// use. The adapter publishes it as a [`Raster`] after the node returns and
/// rejects an output written on both sides or neither.
pub struct Write<'a, P: Payload, R = Any> {
    pub desc: &'a P::Desc,
    pending: &'a mut Pending<P>,
    requirement: PhantomData<R>,
}
struct Pending<P: Payload> {
    host: Option<P::Host>,
    // Kernels write device storage through a handle, so `device` takes
    // `&self`; host storage is written through `&mut`.
    device: OnceCell<Raster<P>>,
}
impl<P: Payload, R> Write<'_, P, R> {
    /// Host storage, allocated with [`Payload::allocate`] on the first call.
    pub fn host(&mut self) -> &mut P::Host {
        self.pending.host.get_or_insert_with(|| P::allocate(self.desc))
    }
}
impl<P: DeviceLayout, R> Write<'_, P, R> {
    /// Uninitialized device storage of `P::byte_len(desc)` bytes for kernels to fill.
    pub fn device(&self, ctx: &KernelContext<'_>) -> Result<&DeviceBuffer<P>> {
        let raster = match self.pending.device.get() {
            Some(raster) => raster,
            None => {
                let buffer = ctx.device_buffer::<P>(self.desc)?;
                let raster = Raster::from_device(self.desc.clone(), buffer, ctx)?;
                self.pending.device.get_or_init(|| raster)
            }
        };
        raster.device(ctx)
    }
}

/// Output storage owned by the generated node adapter until publication.
#[doc(hidden)]
pub struct OutputSlot<P: Payload> {
    desc: Arc<P::Desc>,
    pending: Pending<P>,
}
impl<P: Payload> OutputSlot<P> {
    pub fn new(desc: impl Into<Arc<P::Desc>>) -> Self {
        Self { desc: desc.into(), pending: Pending { host: None, device: OnceCell::new() } }
    }
    pub fn write<R>(&mut self) -> Write<'_, P, R> {
        Write { desc: &self.desc, pending: &mut self.pending, requirement: PhantomData }
    }
    pub fn finish(self) -> Result<Image> {
        match (self.pending.host, self.pending.device.into_inner()) {
            (Some(host), None) => Ok(Raster::<P>::from_host(self.desc, host)?.into()),
            (None, Some(raster)) => Ok(raster.into()),
            (None, None) => Err(Error::Contract("node did not write its output".into())),
            (Some(_), Some(_)) => {
                Err(Error::Contract("node wrote its output on both host and device".into()))
            }
        }
    }
}

/// An output published from samples the node already holds, typically an
/// input under the contract's new description. Published rasters are never
/// written again, so sharing them keeps nodes pure.
pub struct Forward<'a, P: Payload, R = Any> {
    desc: &'a Arc<P::Desc>,
    slot: &'a mut Option<Raster<P>>,
    requirement: PhantomData<R>,
}
impl<P: DeviceLayout, R> Forward<'_, P, R> {
    /// Publish `raster`'s samples as this output ([`Raster::share_as`]).
    pub fn set<Q: DeviceLayout>(self, raster: &Raster<Q>) -> Result<()> {
        *self.slot = Some(raster.share_as(self.desc.clone())?);
        Ok(())
    }
}

/// A forwarded output held by the generated node adapter until publication.
#[doc(hidden)]
pub struct ForwardSlot<P: Payload> {
    desc: Arc<P::Desc>,
    raster: Option<Raster<P>>,
}
impl<P: Payload> ForwardSlot<P> {
    pub fn new(desc: impl Into<Arc<P::Desc>>) -> Self {
        Self { desc: desc.into(), raster: None }
    }
    pub fn forward<R>(&mut self) -> Forward<'_, P, R> {
        Forward { desc: &self.desc, slot: &mut self.raster, requirement: PhantomData }
    }
    pub fn finish(self) -> Result<Image> {
        let raster =
            self.raster.ok_or_else(|| Error::Contract("node did not set its output".into()))?;
        Ok(raster.into())
    }
}

/// An output of a foreign node, which has no contract: the node sets it once
/// with its description and host samples. Setting it validates both.
pub struct Publish<'a, P> {
    slot: &'a mut Option<Image>,
    payload: PhantomData<P>,
}
impl<'a, P: Payload> Publish<'a, P> {
    #[doc(hidden)]
    pub fn new(slot: &'a mut Option<Image>) -> Self {
        Self { slot, payload: PhantomData }
    }
    pub fn set(self, desc: P::Desc, host: P::Host) -> Result<()> {
        P::validate(&desc)?;
        *self.slot = Some(Raster::<P>::from_host(desc, host)?.into());
        Ok(())
    }
}

/// Check a connected input against its port requirement before the contract.
#[doc(hidden)]
pub fn input<'a, P: Payload, R: Requirement<P>>(
    port: &str,
    desc: Option<&'a Description>,
) -> Result<Option<&'a P::Desc>> {
    desc.map(|desc| {
        let desc = downcast(desc);
        R::check(desc).map_err(|e| requirement_error(port, R::LABEL, e))?;
        Ok(desc)
    })
    .transpose()
}
/// Validate a contract's output and its guaranteed interpretation.
#[doc(hidden)]
pub fn output<P: Payload, R: Requirement<P>>(port: &str, desc: P::Desc) -> Result<Description> {
    P::validate(&desc)?;
    R::check(&desc).map_err(|e| requirement_error(port, R::LABEL, e))?;
    Ok(Arc::new(desc))
}
fn requirement_error(port: &str, label: &str, error: Error) -> Error {
    Error::Contract(format!("{port} requires {label}: {error}"))
}

#[doc(hidden)]
pub fn downcast<T: std::any::Any>(desc: &Description) -> &T {
    desc.downcast_ref().expect("typed node adapter invariant")
}
/// A contract's output description as the typed `Arc` an output slot shares.
#[doc(hidden)]
pub fn typed<T: std::any::Any + Send + Sync>(desc: &Description) -> Arc<T> {
    desc.clone().downcast().expect("typed node adapter invariant")
}

#[derive(Clone, Copy)]
pub struct PortSpec {
    pub name: &'static str,
    /// The payload's declared name.
    pub payload: &'static str,
    /// The port's requirement label; empty when it accepts any description.
    pub requirement: &'static str,
    pub optional: bool,
    pub(crate) payload_type: TypeId,
}
impl PortSpec {
    pub const fn of<P: Payload, R: Requirement<P>>(name: &'static str) -> Self {
        Self {
            name,
            payload: P::NAME,
            requirement: R::LABEL,
            optional: false,
            payload_type: TypeId::of::<P>(),
        }
    }
    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
    pub fn is<P: Payload>(&self) -> bool {
        self.payload_type == TypeId::of::<P>()
    }
    /// Payload name and requirement, as help and the editor show a port.
    pub fn label(&self) -> String {
        if self.requirement.is_empty() {
            self.payload.into()
        } else {
            format!("{} · {}", self.payload, self.requirement)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub(crate) graph: u64,
    pub(crate) index: usize,
    pub(crate) generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortId {
    pub(crate) node: NodeId,
    pub(crate) index: usize,
    pub(crate) direction: Direction,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Input,
    Output,
}
pub struct Input<P> {
    pub(crate) id: PortId,
    marker: PhantomData<fn() -> P>,
}
pub struct Output<P> {
    pub(crate) id: PortId,
    marker: PhantomData<fn() -> P>,
}
macro_rules! handle {
    ($t:ident, $direction:ident) => {
        impl<P> Copy for $t<P> {}
        impl<P> Clone for $t<P> {
            fn clone(&self) -> Self {
                *self
            }
        }
        impl<P> $t<P> {
            #[doc(hidden)]
            pub fn new(node: NodeId, index: usize) -> Self {
                Self {
                    id: PortId { node, index, direction: Direction::$direction },
                    marker: PhantomData,
                }
            }
            pub fn id(self) -> PortId {
                self.id
            }
            pub fn node(self) -> NodeId {
                self.id.node
            }
        }
    };
}
handle!(Input, Input);
handle!(Output, Output);

pub(crate) fn wrong_handle() -> Error {
    Error::Graph("foreign, stale, or wrong-direction port handle".into())
}

/// Port identity fields cannot be forged by deserialization.
///
/// ```compile_fail
/// use drip::ports::NodeId;
/// let id = NodeId { graph: 1, index: 0, generation: 1 };
/// ```
const _: () = ();

impl NodeId {
    pub fn index(self) -> usize {
        self.index
    }
}
impl PortId {
    pub fn direction(self) -> Direction {
        self.direction
    }
    pub fn node(self) -> NodeId {
        self.node
    }
    pub fn index(self) -> usize {
        self.index
    }
}
