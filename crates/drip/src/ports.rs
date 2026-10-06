//! Port requirements and tuple adapters. `Read<SceneRec2020>` asks for an exact
//! type; `Read<dyn LinearThreeChannelMatrix>` asks for a capability. Both derive
//! connection checks and input borrows from the same evidence, so kernels need
//! no fallible type switches. Tuple order defines socket and argument order.
//! [`Optional`] wraps either form for inputs a kernel can do without.

use crate::image::{
    ColorspaceRgbMatrix, LinearThreeChannelMatrix, Rec2020, RgbIn, ThreeChannelMatrix,
};
use crate::value::{EdgeValue, TypeDescriptor, Value};
use std::{any::TypeId, marker::PhantomData, sync::Arc};

#[derive(Clone, Copy)]
pub struct InputRequirement {
    pub name: &'static str,
    /// Evaluation proceeds without a source; a connected source must still succeed.
    pub optional: bool,
    pub(crate) accepts: fn(&TypeDescriptor) -> bool,
}
impl InputRequirement {
    pub fn accepts(&self, output: &TypeDescriptor) -> bool {
        (self.accepts)(output)
    }
}

/// A signature-level requirement, not stored image data. `T` is either a
/// concrete edge type or a supported capability trait object.
pub struct Read<T: ?Sized>(PhantomData<T>);
/// An input borrowed as `None` while unconnected.
pub struct Optional<I>(PhantomData<I>);
/// A connection predicate and its corresponding runtime borrow.
///
/// # Laws
/// For every value whose descriptor satisfies `REQUIREMENT`, `read` must succeed
/// and return the promised interpretation of that same payload. Callers check
/// the requirement first. The built-in `Read` implementations derive both
/// operations from the same descriptor evidence to keep this obligation local.
/// `read_slot` receives `None` only if `REQUIREMENT.optional`.
pub trait Input {
    type Borrowed<'value>;
    const REQUIREMENT: InputRequirement;
    fn read(value: &Value) -> Self::Borrowed<'_>;
    fn read_slot(value: Option<&Value>) -> Self::Borrowed<'_> {
        Self::read(value.expect("graph validated required input"))
    }
}
impl<T> Input for Read<T>
where
    T: EdgeValue,
{
    type Borrowed<'value> = &'value T;
    const REQUIREMENT: InputRequirement = InputRequirement {
        name: T::TYPE.name,
        optional: false,
        accepts: |output| (output.id)() == TypeId::of::<T>(),
    };
    fn read(value: &Value) -> &T {
        value.downcast_ref().expect("graph validated exact input")
    }
}
macro_rules! capability_input {
    ($capability:ty, $projection:ident) => {
        impl Input for Read<$capability> {
            type Borrowed<'value> = &'value $capability;
            const REQUIREMENT: InputRequirement = InputRequirement {
                name: stringify!($capability),
                optional: false,
                accepts: |output| output.$projection.is_some(),
            };
            fn read(value: &Value) -> Self::Borrowed<'_> {
                (value.descriptor.$projection.expect("graph validated capability"))(&*value.payload)
            }
        }
    };
}
capability_input!(dyn ThreeChannelMatrix, channels);
capability_input!(dyn LinearThreeChannelMatrix, linear);
capability_input!(dyn ColorspaceRgbMatrix, color);
capability_input!(dyn RgbIn<Rec2020>, rec2020);

impl<I: Input> Input for Optional<I> {
    type Borrowed<'value> = Option<I::Borrowed<'value>>;
    const REQUIREMENT: InputRequirement = InputRequirement { optional: true, ..I::REQUIREMENT };
    fn read(value: &Value) -> Self::Borrowed<'_> {
        Some(I::read(value))
    }
    fn read_slot(value: Option<&Value>) -> Self::Borrowed<'_> {
        value.map(I::read)
    }
}

/// Ordered input requirements and their borrows.
///
/// # Laws
/// `read` must preserve requirement order and arity. Its precondition is exactly
/// `REQUIREMENTS.len()` slots, each satisfying the corresponding requirement
/// and empty only if it is optional.
/// The tuple implementations below derive both sides from the same elements.
pub trait InputTuple {
    type Borrowed<'value>;
    const REQUIREMENTS: &'static [InputRequirement];
    fn read(values: &[Option<Value>]) -> Self::Borrowed<'_>;
}
/// Ordered output descriptors and their erased payloads.
///
/// # Laws
/// `erase` must preserve tuple order and arity; each resulting value must carry
/// the corresponding descriptor in `TYPES`. It must preserve payload semantics.
pub trait OutputTuple {
    const TYPES: &'static [&'static TypeDescriptor];
    fn erase(self) -> Vec<Value>;
}
impl InputTuple for () {
    type Borrowed<'value> = ();
    const REQUIREMENTS: &'static [InputRequirement] = &[];
    fn read(_: &[Option<Value>]) {}
}
impl OutputTuple for () {
    const TYPES: &'static [&'static TypeDescriptor] = &[];
    fn erase(self) -> Vec<Value> {
        vec![]
    }
}
macro_rules! port_tuple {
    ($($element:ident : $index:tt),+) => {
        impl<$($element: Input),+> InputTuple for ($($element,)+) {
            type Borrowed<'value> = ($($element::Borrowed<'value>,)+);
            const REQUIREMENTS: &'static [InputRequirement] = &[$($element::REQUIREMENT),+];
            fn read(values: &[Option<Value>]) -> Self::Borrowed<'_> {
                ($($element::read_slot(values[$index].as_ref()),)+)
            }
        }
        impl<$($element: EdgeValue),+> OutputTuple for ($(Arc<$element>,)+) {
            const TYPES: &'static [&'static TypeDescriptor] = &[$(&$element::TYPE),+];
            fn erase(self) -> Vec<Value> { vec![$(Value::new(self.$index)),+] }
        }
    };
}
port_tuple!(First: 0);
port_tuple!(First: 0, Second: 1);
port_tuple!(First: 0, Second: 1, Third: 2);
port_tuple!(First: 0, Second: 1, Third: 2, Fourth: 3);
