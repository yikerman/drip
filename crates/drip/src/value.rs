//! Immutable erased values retain their logical type and generated capability
//! evidence. Storage identity and semantic identity are checked independently.

use crate::image::{Interpretation, RawMat, RealMat};
use bevy_reflect::{Reflect, TypeRegistration};
use std::{
    any::{Any, TypeId},
    fmt,
    sync::Arc,
};

type Erased = dyn Any + Send + Sync;

#[derive(Clone, Copy)]
pub struct TypeDescriptor {
    pub name: &'static str,
    id: fn() -> TypeId,
    layout: fn() -> TypeId,
    dictionary: fn() -> Option<Arc<TypeRegistration>>,
}
impl TypeDescriptor {
    pub const fn of<T: EdgeValue>() -> Self {
        Self { name: T::NAME, id: TypeId::of::<T>, layout: T::layout, dictionary: T::dictionary }
    }
    pub fn is<T: 'static>(&self) -> bool {
        (self.id)() == TypeId::of::<T>()
    }
    pub fn has_layout<T: 'static>(&self) -> bool {
        (self.layout)() == TypeId::of::<T>()
    }
    pub fn dictionary(&self) -> Option<Arc<TypeRegistration>> {
        (self.dictionary)()
    }
}
impl PartialEq for TypeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        (self.id)() == (other.id)()
    }
}
impl Eq for TypeDescriptor {}
impl fmt::Debug for TypeDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// Local payload implementation; its identity always comes from the actual T.
/// Published values are immutable and obey their documented interpretation laws.
pub trait EdgeValue: Any + Send + Sync + fmt::Debug {
    const NAME: &'static str;
    fn layout() -> TypeId {
        TypeId::of::<Self>()
    }
    fn dictionary() -> Option<Arc<TypeRegistration>> {
        None
    }
    fn matrix(&self) -> Option<MatrixParts> {
        None
    }
}

/// Generated/typed matrix erasure; fields are private so callers cannot supply
/// an unrelated dictionary, interpretation or reconstruction adapter.
#[derive(Clone)]
pub struct MatrixParts {
    pub(crate) storage: Arc<Erased>,
    pub(crate) interpretation: Arc<dyn Reflect>,
    rebuild: fn(Arc<Erased>, Arc<dyn Reflect>) -> Arc<Erased>,
}
impl<const C: usize, I: Interpretation> EdgeValue for RealMat<C, I> {
    const NAME: &'static str = I::NAME;
    fn layout() -> TypeId {
        TypeId::of::<RawMat<C>>()
    }
    fn dictionary() -> Option<Arc<TypeRegistration>> {
        Some(I::dictionary())
    }
    fn matrix(&self) -> Option<MatrixParts> {
        Some(MatrixParts {
            storage: self.data.clone(),
            interpretation: self.interpretation.clone(),
            rebuild: |storage, interpretation| {
                let interpretation: Arc<Erased> = interpretation;
                Arc::new(Self {
                    data: storage.downcast().expect("typed matrix layout"),
                    interpretation: interpretation.downcast().expect("typed interpretation"),
                })
            },
        })
    }
}

#[derive(Clone)]
pub struct Value {
    descriptor: TypeDescriptor,
    payload: Arc<Erased>,
    pub(crate) matrix: Option<MatrixParts>,
}
impl Value {
    pub fn new<T: EdgeValue>(payload: Arc<T>) -> Self {
        Self { descriptor: TypeDescriptor::of::<T>(), matrix: payload.matrix(), payload }
    }
    pub fn descriptor(&self) -> &TypeDescriptor {
        &self.descriptor
    }
    pub fn downcast_ref<T: EdgeValue>(&self) -> Option<&T> {
        self.payload.downcast_ref()
    }
    pub fn downcast<T: EdgeValue>(&self) -> Option<Arc<T>> {
        self.payload.clone().downcast().ok()
    }
    pub fn borrow<R: crate::ports::Input>(&self) -> Option<R::Borrowed<'_>> {
        R::REQUIREMENT.accepts(&self.descriptor).then(|| R::read(self))
    }
    pub(crate) fn preserves(&self, source: &Self) -> bool {
        self.descriptor == source.descriptor
            && match (&self.matrix, &source.matrix) {
                (Some(output), Some(input)) => {
                    Arc::ptr_eq(&output.interpretation, &input.interpretation)
                }
                _ => false,
            }
    }
    pub(crate) fn with_matrix<const C: usize>(&self, data: Arc<RawMat<C>>) -> Self {
        let parts = self.matrix.as_ref().expect("matrix input");
        debug_assert!(self.descriptor.has_layout::<RawMat<C>>());
        debug_assert_eq!(data.width.checked_mul(data.height), Some(data.pixels.len()));
        let payload = (parts.rebuild)(data.clone(), parts.interpretation.clone());
        Self {
            descriptor: self.descriptor,
            payload,
            matrix: Some(MatrixParts { storage: data, ..parts.clone() }),
        }
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(self.descriptor.name).finish_non_exhaustive()
    }
}
