//! Typed views at nodes; type erasure is confined to the graph adapter.
use crate::{Error, Result, payload::Payload, runtime::RuntimeContext};
use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    sync::Arc,
};

pub type Description = Arc<dyn Any + Send + Sync>;
pub type Data = Arc<dyn Any + Send + Sync>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Placement {
    Cpu,
    Device,
}
pub struct Cpu<P>(PhantomData<P>);
pub struct Device<P>(PhantomData<P>);

pub trait Port: Send + Sync + 'static {
    type Payload: Payload;
    type Storage: Send + Sync + 'static;
    const PLACEMENT: Placement;
    fn validate(desc: &<Self::Payload as Payload>::Desc, data: &Self::Storage) -> Result<()>;
    fn allocate(
        desc: &<Self::Payload as Payload>::Desc,
        runtime: &RuntimeContext,
    ) -> Result<Self::Storage>;
    fn transport(
        value: &Data,
        from: Placement,
        desc: &<Self::Payload as Payload>::Desc,
        runtime: &RuntimeContext,
    ) -> Result<Data>;
}
impl<P: Payload> Port for Cpu<P> {
    type Payload = P;
    type Storage = P::Cpu;
    const PLACEMENT: Placement = Placement::Cpu;
    fn validate(desc: &P::Desc, data: &Self::Storage) -> Result<()> {
        P::validate_cpu(desc, data)
    }
    fn allocate(d: &P::Desc, _: &RuntimeContext) -> Result<Self::Storage> {
        Ok(P::allocate_cpu(d))
    }
    fn transport(v: &Data, from: Placement, d: &P::Desc, r: &RuntimeContext) -> Result<Data> {
        match from {
            Placement::Cpu => Ok(v.clone()),
            Placement::Device => Ok(Arc::new(P::download(d, downcast(v), r)?)),
        }
    }
}
impl<P: Payload> Port for Device<P> {
    type Payload = P;
    type Storage = P::Device;
    const PLACEMENT: Placement = Placement::Device;
    fn validate(_: &P::Desc, _: &Self::Storage) -> Result<()> {
        Ok(())
    }
    fn allocate(d: &P::Desc, r: &RuntimeContext) -> Result<Self::Storage> {
        P::allocate_device(d, r)
    }
    fn transport(v: &Data, from: Placement, d: &P::Desc, r: &RuntimeContext) -> Result<Data> {
        match from {
            Placement::Device => Ok(v.clone()),
            Placement::Cpu => Ok(Arc::new(P::upload(d, downcast(v), r)?)),
        }
    }
}

pub struct Read<'a, P: Port> {
    pub data: &'a P::Storage,
    pub desc: &'a <P::Payload as Payload>::Desc,
}
pub struct Write<'a, P: Port> {
    pub data: &'a mut P::Storage,
    pub desc: &'a <P::Payload as Payload>::Desc,
}

#[doc(hidden)]
pub fn downcast<T: Any>(value: &Arc<dyn Any + Send + Sync>) -> &T {
    value.downcast_ref().expect("typed node adapter invariant")
}
#[doc(hidden)]
pub fn description<P: Payload>(value: &Option<Description>) -> Option<&P::Desc> {
    value.as_ref().map(downcast)
}
#[doc(hidden)]
pub fn erase<P: Payload>(desc: Option<P::Desc>) -> Result<Option<Description>> {
    desc.map(|d| {
        P::validate_desc(&d)?;
        Ok(Arc::new(d) as Description)
    })
    .transpose()
}

#[derive(Clone)]
pub struct PortSpec {
    pub name: &'static str,
    pub payload_name: &'static str,
    pub placement: Placement,
    pub optional: bool,
    pub(crate) payload: TypeId,
    pub(crate) transport: fn(&Data, Placement, &Description, &RuntimeContext) -> Result<Data>,
}
impl PortSpec {
    pub fn named_payload(mut self, name: &'static str) -> Self {
        self.payload_name = name;
        self
    }
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
    pub fn is<P: Payload>(&self) -> bool {
        self.payload == TypeId::of::<P>()
    }
    pub fn of<P: Port>(name: &'static str) -> Self {
        Self {
            name,
            payload_name: std::any::type_name::<P::Payload>(),
            placement: P::PLACEMENT,
            optional: false,
            payload: TypeId::of::<P::Payload>(),
            transport: |v, from, d, r| P::transport(v, from, downcast(d), r),
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
pub(crate) enum Direction {
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

/// Nominal payloads cannot be connected through the typed API merely because
/// their storage layouts match.
///
/// ```compile_fail
/// use drip::{graph::Dag, node::data::{CameraRgb, ColorRgb}, ports::{Input, Output}};
/// fn wrong(dag: &mut Dag, camera: Output<CameraRgb>, color: Input<ColorRgb>) {
///     dag.connect(camera, color).unwrap();
/// }
/// ```
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
    pub fn node(self) -> NodeId {
        self.node
    }
    pub fn index(self) -> usize {
        self.index
    }
}
