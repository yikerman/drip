//! Immutable values at the heterogeneous graph boundary.
//!
//! Exact Rust type identity governs binding. A logical family identifies only
//! explicitly paired CPU/GPU representations; equal byte layouts or labels do
//! not imply equal meaning. Transfer preserves the payload's interpretation.

use crate::compute::Compute;
use std::{
    any::{Any, TypeId},
    fmt,
    sync::Arc,
};

/// Where numerical data lives. Ordinary records need no device counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Residence {
    Host,
    Cpu,
    Gpu,
}

/// A concrete, immutable port payload. Implementations define interpretation and
/// optional placement adaptation, not arbitrary casts between semantic families.
/// A kernel remains responsible for the physical claims carried in its metadata.
pub trait EdgeValue: Any + Send + Sync + fmt::Debug {
    const NAME: &'static str;
    const RESIDENCE: Residence = Residence::Host;
    fn family() -> TypeId
    where
        Self: Sized,
    {
        TypeId::of::<Self>()
    }
    fn materialize(value: &Value, _: &Compute) -> Result<Value, String>
    where
        Self: Sized,
    {
        if value.descriptor.is::<Self>() {
            Ok(value.clone())
        } else {
            Err(format!("{} has no declared transfer to {}", value.descriptor.name, Self::NAME))
        }
    }
}

#[derive(Clone, Copy)]
pub struct TypeDescriptor {
    pub name: &'static str,
    pub residence: Residence,
    id: fn() -> TypeId,
    family: fn() -> TypeId,
    transfer: fn(&Value, &Compute) -> Result<Value, String>,
}
impl TypeDescriptor {
    pub const fn of<T: EdgeValue>() -> Self {
        Self {
            name: T::NAME,
            residence: T::RESIDENCE,
            id: TypeId::of::<T>,
            family: T::family,
            transfer: T::materialize,
        }
    }
    pub fn is<T: 'static>(&self) -> bool {
        self.type_id() == TypeId::of::<T>()
    }
    pub fn type_id(&self) -> TypeId {
        (self.id)()
    }
    pub fn compatible_with(&self, other: &Self) -> bool {
        (self.family)() == (other.family)()
    }
    /// Adapt only through this family's declared transfer; verify the returned
    /// concrete representation before allowing typed binding.
    pub fn materialize(&self, value: &Value, compute: &Compute) -> Result<Value, String> {
        if self == value.descriptor() {
            return Ok(value.clone());
        }
        if !self.compatible_with(value.descriptor()) {
            return Err(format!(
                "{} and {} have different payload meanings",
                self.name, value.descriptor.name
            ));
        }
        let adapted = (self.transfer)(value, compute)?;
        if self != adapted.descriptor() {
            return Err(format!("transfer did not produce declared representation {}", self.name));
        }
        Ok(adapted)
    }
}
impl PartialEq for TypeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.type_id() == other.type_id()
    }
}
impl Eq for TypeDescriptor {}
impl fmt::Debug for TypeDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.name, self.residence)
    }
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
    /// Borrowing never performs an implicit transfer or semantic conversion.
    pub fn borrow<R: crate::ports::Input>(&self) -> Option<R::Borrowed<'_>> {
        R::REQUIREMENT.types.contains(&self.descriptor).then(|| R::read(self))
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.descriptor.fmt(f)
    }
}
