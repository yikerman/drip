//! Typed port requirements and tuple adapters. Only this boundary erases types;
//! kernels receive concrete borrows or the capability their signature requests.
//!
//! `Read<SceneRec2020>` requires an exact semantic type, whereas
//! `Read<dyn LinearThreeChannelMatrix>` accepts any registered exposure-linear
//! image. Both generate a connection predicate and the matching input borrow.
//! Graph validation checks the predicate before evaluation; adapters can then
//! trust it instead of repeating fallible type switches inside every node.
//!
//! Tuple order determines socket order and evaluator argument order together.
//! The GUI obtains the same requirements from `NodeKind`, so adding a payload
//! that implements a capability does not require editing its consumers.

use crate::image::{
    ColorspaceRgbMatrix, LinearThreeChannelMatrix, Rec2020, RgbIn, ThreeChannelMatrix,
};
use crate::value::{EdgeValue, TypeDescriptor, Value};
use std::{any::TypeId, marker::PhantomData, sync::Arc};

#[derive(Clone, Copy)]
pub struct InputRequirement {
    pub name: &'static str,
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
/// A connection predicate and its corresponding runtime borrow.
///
/// # Laws
/// For every value whose descriptor satisfies `REQUIREMENT`, `read` must succeed
/// and return the promised interpretation of that same payload. Callers check
/// the requirement first. The built-in `Read` implementations derive both
/// operations from the same descriptor evidence to keep this obligation local.
pub trait Input {
    type Borrowed<'value>;
    const REQUIREMENT: InputRequirement;
    fn read(value: &Value) -> Self::Borrowed<'_>;
}
impl<T> Input for Read<T>
where
    T: EdgeValue,
{
    type Borrowed<'value> = &'value T;
    const REQUIREMENT: InputRequirement = InputRequirement {
        name: T::TYPE.name,
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

/// Ordered input requirements and their borrows.
///
/// # Laws
/// `read` must preserve requirement order and arity. Its precondition is exactly
/// `REQUIREMENTS.len()` values, each satisfying the corresponding requirement.
/// The tuple implementations below derive both sides from the same elements.
pub trait InputTuple {
    type Borrowed<'value>;
    const REQUIREMENTS: &'static [InputRequirement];
    fn read(values: &[Value]) -> Self::Borrowed<'_>;
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
    fn read(_: &[Value]) {}
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
            fn read(values: &[Value]) -> Self::Borrowed<'_> {
                ($($element::read(&values[$index]),)+)
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
