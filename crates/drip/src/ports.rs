//! One requirement supplies preflight checking and typed input binding. Optional
//! inputs permit absence, never an incompatible or failed connected source.

use crate::{
    image::RawMat,
    value::{EdgeValue, TypeDescriptor, Value},
};
use bevy_reflect::{Reflect, TypeRegistration};
use std::{marker::PhantomData, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected}, received {actual}: {reason}")]
pub struct TypeMismatch {
    pub expected: &'static str,
    pub actual: &'static str,
    pub reason: &'static str,
}

#[derive(Clone, Copy)]
pub struct InputRequirement {
    pub name: &'static str,
    pub optional: bool,
    check: fn(&TypeDescriptor) -> Result<(), TypeMismatch>,
}
impl InputRequirement {
    pub fn check(&self, output: &TypeDescriptor) -> Result<(), TypeMismatch> {
        (self.check)(output)
    }
    pub fn accepts(&self, output: &TypeDescriptor) -> bool {
        self.check(output).is_ok()
    }
}

/// Implemented by `#[capability]` for its dyn trait, using the generated Bevy adapter.
pub trait Capability {
    const NAME: &'static str;
    fn accepts(registration: &TypeRegistration) -> bool;
    fn project<'a>(registration: &TypeRegistration, value: &'a dyn Reflect) -> Option<&'a Self>;
}

pub struct Read<T>(PhantomData<T>);
pub struct ReadMat<const C: usize, I: ?Sized>(PhantomData<I>);
pub struct Optional<I>(PhantomData<I>);

/// A borrowed layout with the requested semantic evidence; the source retains
/// its concrete interpretation so an explicitly preserving output can reuse it.
pub struct MatRef<'a, const C: usize, I: ?Sized> {
    data: Arc<RawMat<C>>,
    pub interpretation: &'a I,
    source: &'a Value,
}
impl<const C: usize, I: ?Sized> MatRef<'_, C, I> {
    pub fn buffer(&self) -> &Arc<RawMat<C>> {
        &self.data
    }
    /// Declares that the full input interpretation still describes these samples.
    /// INDEX is the input position named by the output's preservation contract.
    pub fn preserve<const INDEX: usize>(&self, pixels: Vec<[f32; C]>) -> Preserved<INDEX> {
        Preserved(self.source.with_matrix(Arc::new(RawMat { pixels, ..*self.data })))
    }
}
impl<I: ?Sized> MatRef<'_, 3, I> {
    pub fn rgb(&self) -> &Arc<RawMat<3>> {
        &self.data
    }
}

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
    const REQUIREMENT: InputRequirement = InputRequirement {
        name: T::NAME,
        optional: false,
        check: |d| {
            if d.is::<T>() {
                Ok(())
            } else {
                Err(TypeMismatch {
                    expected: T::NAME,
                    actual: d.name,
                    reason: "different logical type",
                })
            }
        },
    };
    fn read(value: &Value) -> &T {
        value.downcast_ref().expect("checked logical type")
    }
}
impl<const C: usize, I: Capability + ?Sized + 'static> Input for ReadMat<C, I> {
    type Borrowed<'a> = MatRef<'a, C, I>;
    const REQUIREMENT: InputRequirement = InputRequirement {
        name: I::NAME,
        optional: false,
        check: |d| {
            let reason = if !d.has_layout::<RawMat<C>>() {
                "different matrix layout"
            } else if !d.dictionary().is_some_and(|r| I::accepts(&r)) {
                "interpretation lacks capability"
            } else {
                return Ok(());
            };
            Err(TypeMismatch { expected: I::NAME, actual: d.name, reason })
        },
    };
    fn read(value: &Value) -> Self::Borrowed<'_> {
        let parts = value.matrix.as_ref().expect("checked matrix");
        let registration = value.descriptor().dictionary().expect("checked interpretation");
        // Arc downcasts here recover the original allocation, without copying samples.
        let data = parts.storage.clone().downcast::<RawMat<C>>().expect("checked matrix layout");
        let interpretation = I::project(&registration, parts.interpretation.as_ref())
            .expect("typed immutable dictionary");
        MatRef { data, interpretation, source: value }
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

/// Output contracts resolve either to a fixed type or to an input's type.
/// This lets graph edits check downstream compatibility before running kernels.
#[derive(Debug, Clone, Copy)]
pub enum OutputType {
    Fixed(TypeDescriptor),
    Preserve(usize),
}
impl OutputType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Fixed(t) => t.name,
            Self::Preserve(_) => "same interpretation as input",
        }
    }
}
/// New samples retaining the full interpretation of input `INPUT` (zero-based).
///
/// This expresses the relationship `RealMat<C, I> -> RealMat<C, I>` when the
/// input exposes only a capability and its concrete `I` is erased. For example,
/// exposure accepts any `ScaleInvariant` interpretation; `Preserved<0>` lets the
/// graph propagate that input's logical type before evaluating the pixels.
///
/// Evaluation checks that the output shares the indicated input's interpretation
/// witness, not merely its Rust type: interpretation values may carry calibration
/// data. Construct through [`MatRef::preserve`]. The kernel remains responsible
/// for ensuring the new samples satisfy that interpretation's laws.
#[derive(Debug, Clone)]
pub struct Preserved<const INPUT: usize>(Value);

pub trait Output {
    const TYPE: OutputType;
    fn erase(self) -> Value;
}
impl<T: EdgeValue> Output for Arc<T> {
    const TYPE: OutputType = OutputType::Fixed(TypeDescriptor::of::<T>());
    fn erase(self) -> Value {
        Value::new(self)
    }
}
impl<const N: usize> Output for Preserved<N> {
    const TYPE: OutputType = OutputType::Preserve(N);
    fn erase(self) -> Value {
        self.0
    }
}
pub trait InputTuple {
    type Borrowed<'value>;
    const REQUIREMENTS: &'static [InputRequirement];
    fn read(values: &[Option<Value>]) -> Self::Borrowed<'_>;
}
pub trait OutputTuple {
    const TYPES: &'static [OutputType];
    fn erase(self) -> Vec<Value>;
}
impl InputTuple for () {
    type Borrowed<'a> = ();
    const REQUIREMENTS: &'static [InputRequirement] = &[];
    fn read(_: &[Option<Value>]) {}
}
impl OutputTuple for () {
    const TYPES: &'static [OutputType] = &[];
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
            const TYPES: &'static [OutputType] = &[$($t::TYPE),+];
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
