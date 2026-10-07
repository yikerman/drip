//! Concrete port contracts shared by graph edits and typed executor binding.
//! Optional inputs permit absence, never an incompatible or failed source.
use crate::value::{EdgeValue, TypeDescriptor, Value};
use std::{marker::PhantomData, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected}, received {actual}")]
pub struct TypeMismatch {
    pub expected: String,
    pub actual: &'static str,
}

#[derive(Clone, Copy)]
pub struct InputRequirement {
    pub types: &'static [TypeDescriptor],
    pub optional: bool,
}
impl InputRequirement {
    pub fn name(&self) -> String {
        self.types.iter().map(|ty| ty.name).collect::<Vec<_>>().join(" or ")
    }
    pub fn accepts(&self, output: &TypeDescriptor) -> bool {
        self.types.contains(output)
    }
    pub fn check(&self, output: &TypeDescriptor) -> Result<(), TypeMismatch> {
        if self.accepts(output) {
            Ok(())
        } else {
            Err(TypeMismatch { expected: self.name(), actual: output.name })
        }
    }
}

pub struct Read<T>(PhantomData<T>);
pub struct Optional<I>(PhantomData<I>);

/// An explicit choice between concrete inputs, for consumers such as channel
/// scopes that handle camera RGB as well as working RGB. It does not propagate
/// an unknown output type: every node still declares concrete outputs.
#[derive(Debug, Clone, Copy)]
pub enum Either<A, B> {
    First(A),
    Second(B),
}
pub struct ReadEither<A, B>(PhantomData<(A, B)>);

pub trait Input {
    type Borrowed<'value>;
    const REQUIREMENT: InputRequirement;
    fn read(value: &Value) -> Self::Borrowed<'_>;
    fn read_slot(value: Option<&Value>) -> Self::Borrowed<'_> {
        Self::read(value.expect("validated required input"))
    }
}
impl<T: EdgeValue> Input for Read<T> {
    type Borrowed<'a> = &'a T;
    const REQUIREMENT: InputRequirement =
        InputRequirement { types: &[TypeDescriptor::of::<T>()], optional: false };
    fn read(value: &Value) -> &T {
        value.downcast_ref().expect("checked input type")
    }
}
impl<A: EdgeValue, B: EdgeValue> Input for ReadEither<A, B> {
    type Borrowed<'a> = Either<&'a A, &'a B>;
    const REQUIREMENT: InputRequirement = InputRequirement {
        types: &[TypeDescriptor::of::<A>(), TypeDescriptor::of::<B>()],
        optional: false,
    };
    fn read(value: &Value) -> Self::Borrowed<'_> {
        match value.downcast_ref::<A>() {
            Some(a) => Either::First(a),
            None => Either::Second(value.downcast_ref().expect("checked alternative input type")),
        }
    }
}
impl<I: Input> Input for Optional<I> {
    type Borrowed<'a> = Option<I::Borrowed<'a>>;
    const REQUIREMENT: InputRequirement = InputRequirement { optional: true, ..I::REQUIREMENT };
    fn read(value: &Value) -> Self::Borrowed<'_> {
        Some(I::read(value))
    }
    fn read_slot(value: Option<&Value>) -> Self::Borrowed<'_> {
        value.map(I::read)
    }
}

pub trait Output {
    const TYPE: TypeDescriptor;
    fn erase(self) -> Value;
}
impl<T: EdgeValue> Output for Arc<T> {
    const TYPE: TypeDescriptor = TypeDescriptor::of::<T>();
    fn erase(self) -> Value {
        Value::new(self)
    }
}
pub trait InputTuple {
    type Borrowed<'value>;
    const REQUIREMENTS: &'static [InputRequirement];
    fn read(values: &[Option<Value>]) -> Self::Borrowed<'_>;
}
pub trait OutputTuple {
    const TYPES: &'static [TypeDescriptor];
    fn erase(self) -> Vec<Value>;
}
impl InputTuple for () {
    type Borrowed<'a> = ();
    const REQUIREMENTS: &'static [InputRequirement] = &[];
    fn read(_: &[Option<Value>]) {}
}
impl OutputTuple for () {
    const TYPES: &'static [TypeDescriptor] = &[];
    fn erase(self) -> Vec<Value> {
        vec![]
    }
}
macro_rules! tuples {
    ($($t:ident : $n:tt),+) => {
        impl<$($t: Input),+> InputTuple for ($($t,)+) {
            type Borrowed<'a> = ($($t::Borrowed<'a>,)+);
            const REQUIREMENTS: &'static [InputRequirement] = &[$($t::REQUIREMENT),+];
            fn read(v: &[Option<Value>]) -> Self::Borrowed<'_> { ($($t::read_slot(v[$n].as_ref()),)+) }
        }
        impl<$($t: Output),+> OutputTuple for ($($t,)+) {
            const TYPES: &'static [TypeDescriptor] = &[$($t::TYPE),+];
            fn erase(self) -> Vec<Value> { vec![$(self.$n.erase()),+] }
        }
    };
}
tuples!(A:0);
tuples!(A:0,B:1);
tuples!(A:0,B:1,C:2);
tuples!(A:0,B:1,C:2,D:3);
tuples!(A:0,B:1,C:2,D:3,E:4);
tuples!(A:0,B:1,C:2,D:3,E:4,F:5);
