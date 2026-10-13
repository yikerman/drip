//! How evaluation runs a node kind, and what the segment compiler composes.
//!
//! Every [`Kind`](crate::definition::Kind) declares a [`Shape`]. A kernel or
//! source runs as written. A map registers a [`MapBody`], a `#[cube]` function
//! of one [`Pixel`]. A warp returns a [`Warp`], projective maps between planes,
//! and a resample names a [`FilterBody`]. `plan` composes them into one kernel
//! per segment instead of launching one per node.
use crate::payload::DeviceLayout;
use cubecl::prelude::*;

/// How evaluation runs a node kind. The segment compiler fuses maps, warps
/// and resamples. `Kernel` and `Source` stay opaque to it.
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
    /// The node's host function returns a [`Warp`], and its output is a
    /// deferred image resampled once, together with neighbouring warps.
    Warp,
    /// The node's host function returns a [`FilterBody`], and its output is
    /// its input resampled with that filter onto the output's lattice.
    Resample,
}

/// One sample of a [`MapPayload`] as a map body sees it, channels in storage order.
#[derive(CubeType, Clone, Copy)]
pub struct Pixel {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// A payload the fused kernel reads and writes: packed `[f32; 3]` samples,
/// one [`Pixel`] each, on the [`Lattice`] its description states.
pub trait MapPayload: DeviceLayout {
    fn lattice(desc: &Self::Desc) -> Lattice;
}

/// Where stored samples sit. `extent` counts samples per axis, and `full` is
/// the size of the plane they cover in full-detail pixels. Sample `i` sits
/// at plane position `(i + 0.5) · full / extent` along each axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lattice {
    pub extent: [u32; 2],
    pub full: [u32; 2],
}

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

/// A projective map of the plane on homogeneous `[x, y, 1]` columns.
pub type Projective = [[f64; 3]; 3];

/// What a warp's host function returns. `maps` take the output plane to the
/// input plane, per channel, in each description's full-detail pixels
/// (`[0, full]` along each axis). Output points whose homogeneous `w` is not
/// positive, or that land outside the input plane, are black.
#[derive(Clone, Debug, PartialEq)]
pub struct Warp {
    pub maps: [Projective; 3],
    /// The filter that reconstructs the input where the maps move samples
    /// off the input lattice and no Resample chooses one. `None` declares a
    /// reindex: every output sample lands on an input sample, which
    /// evaluation checks.
    pub filter: Option<&'static FilterBody>,
}
impl Warp {
    /// One map for every channel that moves samples onto samples only, such
    /// as a reflection or quarter turn of the whole plane.
    pub fn reindex(map: Projective) -> Self {
        Self { maps: [map; 3], filter: None }
    }
    /// Maps that may move samples between lattice positions, reconstructed
    /// with `filter`.
    pub fn filtered(maps: [Projective; 3], filter: &'static FilterBody) -> Self {
        Self { maps, filter: Some(filter) }
    }
}

/// The expansion of a filter kernel `#[cube] fn(x: f32) -> f32`, the weight
/// at signed distance `x` in filter units.
pub type FilterExpand = fn(&Scope, <f32 as CubeType>::ExpandType) -> <f32 as CubeType>::ExpandType;

/// A resampling filter as the fused kernel calls it. Kernel identity
/// compares filters by name.
#[derive(Clone, Copy)]
pub struct FilterBody {
    /// Unique among filters.
    pub name: &'static str,
    /// Weights are zero for `|x|` beyond this, in filter units.
    pub support: f32,
    /// The weight is one at zero and zero at every other integer, so
    /// resampling at sample positions reproduces the samples. Evaluation then
    /// copies samples instead of filtering them.
    pub interpolating: bool,
    pub expand: FilterExpand,
}
impl PartialEq for FilterBody {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for FilterBody {}
impl std::hash::Hash for FilterBody {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}
impl std::fmt::Debug for FilterBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}
