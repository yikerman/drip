//! How evaluation runs a node kind, and the bodies the segment compiler composes.
//!
//! Every [`Kind`](crate::definition::Kind) declares a [`Shape`]. A kernel or
//! source runs as written. A map registers a [`MapBody`], a `#[cube]` function
//! of one [`Pixel`], so `plan` can fuse a chain of maps into one kernel
//! instead of launching one per node. Later shapes add their body types here.
use crate::payload::DeviceLayout;
use cubecl::prelude::*;

/// How evaluation runs a node kind. The segment compiler fuses maps, and
/// later warps. `Kernel` and `Source` stay opaque to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The node's function runs on every evaluation. Its inputs are
    /// materialized first, and each output where the node wrote it.
    Kernel,
    /// The node loads its outputs once, when first evaluated, and keeps them
    /// with the admitted node.
    Source,
    /// The node's host function returns constants for its body, and its
    /// output is a deferred image. A consumer that needs samples runs every
    /// map since the last raster in one kernel.
    Map(&'static MapBody),
}

/// One sample of a [`MapPayload`] as a map body sees it, channels in storage order.
#[derive(CubeType, Clone, Copy)]
pub struct Pixel {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// A payload map bodies read: packed `[f32; 3]` samples, one [`Pixel`] each,
/// so a description holds `byte_len / 12` pixels.
pub trait MapPayload: DeviceLayout {}

/// The expansion of a map body `#[cube] fn(px: Pixel, k: &[f32]) -> Pixel`,
/// which CubeCL generates as `body::expand`. `k` holds the body's constants.
pub type Expand = fn(
    &Scope,
    <Pixel as CubeType>::ExpandType,
    &<[f32] as CubeType>::ExpandType,
) -> <Pixel as CubeType>::ExpandType;

/// What a map kind registers: its body and how many constants the body reads.
/// `#[node(map = body)]` generates it.
#[derive(Clone, Copy)]
pub struct MapBody {
    /// Unique within the binary: the declaring module and the body's path.
    /// Kernel identity compares bodies by name and constant count.
    pub name: &'static str,
    pub constants: usize,
    pub expand: Expand,
}
impl PartialEq for MapBody {
    fn eq(&self, other: &Self) -> bool {
        (self.name, self.constants) == (other.name, other.constants)
    }
}
impl Eq for MapBody {}
impl std::hash::Hash for MapBody {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.name, self.constants).hash(state);
    }
}
impl std::fmt::Debug for MapBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}]", self.name, self.constants)
    }
}

/// The constants a map's host function returns: `[f32; N]`, read by the body
/// as `k[0..N]`.
pub trait Constants {
    const LEN: usize;
}
impl<const N: usize> Constants for [f32; N] {
    const LEN: usize = N;
}
