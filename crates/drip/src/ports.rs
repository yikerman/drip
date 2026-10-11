//! Typed views at nodes; type erasure is confined to the graph adapter.
use crate::{
    Error, Result,
    payload::{self, DeviceBuffer, DeviceLayout, Payload},
    runtime::RuntimeContext,
};
use std::{any::TypeId, marker::PhantomData, sync::Arc};

pub type Description = Arc<dyn std::any::Any + Send + Sync>;
pub type Data = Arc<dyn std::any::Any + Send + Sync>;
pub struct Cpu<P>(PhantomData<P>);
pub struct Device<P>(PhantomData<P>);

/// A port's payload and placement. Only payloads with a device layout have a
/// device placement:
///
/// ```compile_fail
/// use drip::{node::data::CaptureMetadata, ports::{Device, Port}};
/// fn port<P: Port>() {}
/// port::<Device<CaptureMetadata>>();
/// ```
pub trait Port: Send + Sync + 'static {
    type Payload: Payload;
    type Storage: Send + Sync + 'static;
    /// Device ports transfer between placements; host ports never transfer.
    const TRANSFERS: Option<Transfers>;
    fn check(desc: &<Self::Payload as Payload>::Desc, data: &Self::Storage) -> Result<()>;
    fn allocate(
        desc: &<Self::Payload as Payload>::Desc,
        runtime: &RuntimeContext,
    ) -> Result<Self::Storage>;
}
impl<P: Payload> Port for Cpu<P> {
    type Payload = P;
    type Storage = P::Host;
    const TRANSFERS: Option<Transfers> = None;
    fn check(desc: &P::Desc, data: &P::Host) -> Result<()> {
        P::check(desc, data)
    }
    fn allocate(desc: &P::Desc, _: &RuntimeContext) -> Result<P::Host> {
        Ok(P::allocate(desc))
    }
}
impl<P: DeviceLayout> Port for Device<P> {
    type Payload = P;
    type Storage = DeviceBuffer<P>;
    const TRANSFERS: Option<Transfers> = Some(Transfers {
        upload: |v, d, r| Ok(Arc::new(payload::upload::<P>(downcast(d), downcast(v), r)?)),
        download: |v, d, r| Ok(Arc::new(payload::download::<P>(downcast(d), downcast(v), r)?)),
    });
    fn check(_: &P::Desc, _: &DeviceBuffer<P>) -> Result<()> {
        Ok(())
    }
    fn allocate(desc: &P::Desc, runtime: &RuntimeContext) -> Result<DeviceBuffer<P>> {
        Ok(DeviceBuffer::new(runtime.client()?.empty(P::byte_len(desc))))
    }
}

type Transfer = fn(&Data, &Description, &RuntimeContext) -> Result<Data>;
/// Host/device movement of one device-capable payload. Evaluation uses the
/// device side of an edge: the consumer's upload or the producer's download.
#[derive(Clone, Copy)]
pub struct Transfers {
    pub(crate) upload: Transfer,
    pub(crate) download: Transfer,
}

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

type Desc<P> = <<P as Port>::Payload as Payload>::Desc;

pub struct Read<'a, P: Port, R = Any> {
    pub data: &'a P::Storage,
    pub desc: &'a Desc<P>,
    requirement: PhantomData<R>,
}
pub struct Write<'a, P: Port, R = Any> {
    pub data: &'a mut P::Storage,
    pub desc: &'a Desc<P>,
    requirement: PhantomData<R>,
}

impl<'a, P: Port, R> Read<'a, P, R> {
    pub fn new(desc: &'a Desc<P>, data: &'a P::Storage) -> Self {
        Self { data, desc, requirement: PhantomData }
    }
    #[doc(hidden)]
    pub fn bind(value: Option<&'a (Description, Data)>) -> Option<Self> {
        value.map(|(desc, data)| Self::new(downcast(desc), downcast(data)))
    }
}

impl<'a, P: Port, R> Write<'a, P, R> {
    pub fn new(desc: &'a Desc<P>, data: &'a mut P::Storage) -> Self {
        Self { data, desc, requirement: PhantomData }
    }
}

/// Fresh output storage owned by the generated node adapter until publication.
#[doc(hidden)]
pub struct OutputSlot<'a, P: Port> {
    desc: &'a Desc<P>,
    data: P::Storage,
}
impl<'a, P: Port> OutputSlot<'a, P> {
    pub fn allocate(
        desc: &'a Description,
        context: &crate::runtime::KernelContext<'_>,
    ) -> Result<Self> {
        let desc = downcast(desc);
        Ok(Self { desc, data: context.allocate::<P>(desc)? })
    }
    pub fn write<R>(&mut self) -> Write<'_, P, R> {
        Write::new(self.desc, &mut self.data)
    }
    pub fn finish(self) -> Result<Data> {
        P::check(self.desc, &self.data)?;
        Ok(Arc::new(self.data))
    }
}

/// An output of a source node, set once when the node is first evaluated.
/// Setting it is the boundary check of the loaded value.
pub struct Load<'a, P> {
    slot: &'a mut Option<(Description, Data)>,
    payload: PhantomData<P>,
}
impl<'a, P: Payload> Load<'a, P> {
    #[doc(hidden)]
    pub fn new(slot: &'a mut Option<(Description, Data)>) -> Self {
        Self { slot, payload: PhantomData }
    }
    pub fn set(self, desc: P::Desc, host: P::Host) -> Result<()> {
        P::validate(&desc)?;
        P::check(&desc, &host)?;
        *self.slot = Some((Arc::new(desc), Arc::new(host)));
        Ok(())
    }
}

/// Check a connected input against its port requirement before the contract.
#[doc(hidden)]
pub fn input<'a, P: Port, R: Requirement<P::Payload>>(
    port: &str,
    desc: Option<&'a Description>,
) -> Result<Option<&'a Desc<P>>> {
    desc.map(|desc| {
        let desc = downcast(desc);
        R::check(desc).map_err(|e| requirement_error(port, R::LABEL, e))?;
        Ok(desc)
    })
    .transpose()
}
/// Validate a contract's output and its guaranteed interpretation.
#[doc(hidden)]
pub fn output<P: Port, R: Requirement<P::Payload>>(
    port: &str,
    desc: Desc<P>,
) -> Result<Description> {
    P::Payload::validate(&desc)?;
    R::check(&desc).map_err(|e| requirement_error(port, R::LABEL, e))?;
    Ok(Arc::new(desc))
}
fn requirement_error(port: &str, label: &str, error: Error) -> Error {
    Error::Contract(format!("{port} requires {label}: {error}"))
}

#[doc(hidden)]
pub fn downcast<T: std::any::Any>(value: &Arc<dyn std::any::Any + Send + Sync>) -> &T {
    value.downcast_ref().expect("typed node adapter invariant")
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
    pub(crate) transfers: Option<Transfers>,
}
impl PortSpec {
    pub const fn of<P: Port, R: Requirement<P::Payload>>(name: &'static str) -> Self {
        Self {
            name,
            payload: P::Payload::NAME,
            requirement: R::LABEL,
            optional: false,
            payload_type: TypeId::of::<P::Payload>(),
            transfers: P::TRANSFERS,
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
