//! Shared edge payloads and the capability evidence retained after type erasure.
//!
//! An editable graph contains heterogeneous nodes and is loaded before images
//! exist. It therefore needs both a runtime description for checking connections
//! and an erased value for caching results. These are two uses of the same
//! [`EdgeValue::TYPE`] declaration, rather than independent port/value enums.
//! [`Describe`] builds the evidence used by both compatibility checks and input
//! borrowing; a node never maintains a second list of accepted image variants.
//!
//! Rust's `Any` can recover a concrete type, but cannot discover which other
//! traits it implements. Capability registration is the explicit bridge: adding
//! an image type registers its strongest exposed capability once; adding a new
//! capability requires a descriptor projection here and a matching `Read`
//! implementation in [`crate::ports`]. Keep those two in sync. Stronger builders
//! include parent capabilities so individual payload registrations need not.

use std::any::{Any, TypeId};
use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::image::{
    CameraRgb, ColorspaceRgbMatrix, DisplayRec2020, LinearThreeChannelMatrix, Mosaic, RawMetadata,
    Rec2020, RgbIn, SceneRec2020, ThreeChannelMatrix,
};

pub(crate) type Erased = dyn Any + Send + Sync;
type Projection<Capability> = for<'a> fn(&'a Erased) -> &'a Capability;

/// Runtime evidence for one concrete edge type; projections borrow capabilities
/// from its existing allocation rather than copying or converting the image.
pub struct TypeDescriptor {
    pub name: &'static str,
    pub(crate) id: fn() -> TypeId,
    pub(crate) channels: Option<Projection<dyn ThreeChannelMatrix>>,
    pub(crate) linear: Option<Projection<dyn LinearThreeChannelMatrix>>,
    pub(crate) color: Option<Projection<dyn ColorspaceRgbMatrix>>,
    pub(crate) rec2020: Option<Projection<dyn RgbIn<Rec2020>>>,
    equal: fn(&Erased, &Erased) -> bool,
}

impl Debug for TypeDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// A registered graph payload. The builder ties all projections to this same
/// concrete type; node input lists never repeat these capabilities.
///
/// # Laws
/// Build `TYPE` with `Describe::<Self>`, and expose only capabilities whose
/// semantic laws the payload obeys. The builder checks trait bounds, not those
/// laws. A payload and its interpretation must remain unchanged while cached;
/// interior mutation would invalidate dependency-stamp reuse.
pub trait EdgeValue: Any + Send + Sync + Debug + PartialEq {
    const TYPE: TypeDescriptor;
}

/// Register capabilities only when the payload has the corresponding trait impl.
/// `T` is the concrete payload; its semantic laws remain an implementer obligation.
///
/// ```compile_fail,E0277
/// use drip::image::Mosaic;
/// use drip::value::Describe;
/// let invalid = Describe::<Mosaic>::new("sensor").rec2020().build();
/// ```
pub struct Describe<T> {
    descriptor: TypeDescriptor,
    payload: PhantomData<T>,
}

// T: concrete edge payload.
impl<T> Describe<T>
where
    T: EdgeValue,
{
    pub const fn new(name: &'static str) -> Self {
        Self {
            descriptor: TypeDescriptor {
                name,
                id: TypeId::of::<T>,
                channels: None,
                linear: None,
                color: None,
                rec2020: None,
                equal: |left, right| left.downcast_ref::<T>() == right.downcast_ref::<T>(),
            },
            payload: PhantomData,
        }
    }
    pub const fn channels(mut self) -> Self
    where
        T: ThreeChannelMatrix,
    {
        self.descriptor.channels =
            Some(|value| value.downcast_ref::<T>().expect("descriptor identifies payload"));
        self
    }
    pub const fn linear(mut self) -> Self
    where
        T: LinearThreeChannelMatrix,
    {
        self = self.channels();
        self.descriptor.linear =
            Some(|value| value.downcast_ref::<T>().expect("descriptor identifies payload"));
        self
    }
    pub const fn color(mut self) -> Self
    where
        T: ColorspaceRgbMatrix,
    {
        self = self.linear();
        self.descriptor.color =
            Some(|value| value.downcast_ref::<T>().expect("descriptor identifies payload"));
        self
    }
    pub const fn rec2020(mut self) -> Self
    where
        T: RgbIn<Rec2020>,
    {
        self = self.color();
        self.descriptor.rec2020 =
            Some(|value| value.downcast_ref::<T>().expect("descriptor identifies payload"));
        self
    }
    pub const fn build(self) -> TypeDescriptor {
        self.descriptor
    }
}

// Built-in payloads expose only their strongest capability; parent registrations
// come from Describe, so this list cannot forget channel access on a color image.
macro_rules! edge_type {
    ($payload:ty $(, $capability:ident)?) => {
        impl EdgeValue for $payload {
            const TYPE: TypeDescriptor = Describe::<Self>::new(stringify!($payload))
                $(.$capability())?.build();
        }
    };
}
edge_type!(Mosaic);
edge_type!(RawMetadata);
edge_type!(CameraRgb, linear);
edge_type!(SceneRec2020, rec2020);
edge_type!(DisplayRec2020, rec2020);

/// An immutable, shared payload at the heterogeneous graph/cache boundary.
/// Erasure preserves its concrete semantic type and allocation identity; it is
/// not a conversion to a common RGB type. Cache stamps describe dependencies,
/// so evaluating cache validity never compares the image pixels here.
#[derive(Clone)]
pub struct Value {
    pub(crate) descriptor: &'static TypeDescriptor,
    pub(crate) payload: Arc<Erased>,
}
impl Value {
    pub fn new<T: EdgeValue>(payload: Arc<T>) -> Self {
        Self { descriptor: &T::TYPE, payload }
    }
    pub fn descriptor(&self) -> &'static TypeDescriptor {
        self.descriptor
    }
    pub fn downcast_ref<T: EdgeValue>(&self) -> Option<&T> {
        self.payload.downcast_ref()
    }
    pub fn downcast<T: EdgeValue>(&self) -> Option<Arc<T>> {
        self.payload.clone().downcast().ok()
    }
    /// Inspects a cached result through an exact type or a registered capability.
    pub fn borrow<Requirement: crate::ports::Input>(&self) -> Option<Requirement::Borrowed<'_>> {
        Requirement::REQUIREMENT.accepts(self.descriptor).then(|| Requirement::read(self))
    }
}
impl Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(self.descriptor.name).finish_non_exhaustive()
    }
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        (self.descriptor.id)() == (other.descriptor.id)()
            && (self.descriptor.equal)(&*self.payload, &*other.payload)
    }
}
