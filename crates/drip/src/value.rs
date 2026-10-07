//! Immutable graph values erased at the executor boundary. The descriptor and
//! payload are constructed together from the same concrete Rust type.
use std::{
    any::{Any, TypeId},
    fmt,
    sync::Arc,
};

#[derive(Clone, Copy)]
pub struct TypeDescriptor {
    pub name: &'static str,
    id: fn() -> TypeId,
}
impl TypeDescriptor {
    pub const fn of<T: EdgeValue>() -> Self {
        Self { name: T::NAME, id: TypeId::of::<T> }
    }
    pub fn is<T: 'static>(&self) -> bool {
        (self.id)() == TypeId::of::<T>()
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

/// A locally defined payload. Names describe contracts; identity comes from T,
/// so two types sharing a label never become interchangeable. Implementations
/// and kernels are responsible for the documented meaning of their values.
pub trait EdgeValue: Any + Send + Sync + fmt::Debug {
    const NAME: &'static str;
}

#[derive(Clone)]
pub struct Value {
    descriptor: TypeDescriptor,
    payload: Arc<dyn Any + Send + Sync>,
}
impl Value {
    pub fn new<T: EdgeValue>(payload: Arc<T>) -> Self {
        Self { descriptor: TypeDescriptor::of::<T>(), payload }
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
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(self.descriptor.name).finish_non_exhaustive()
    }
}
