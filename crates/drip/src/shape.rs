//! How evaluation runs a node kind, and the bodies the segment compiler fuses.
//!
//! Every [`Kind`](crate::definition::Kind) declares a [`Shape`], which
//! `#[node]` derives from the declaration. Kernels, sources and foreign nodes
//! run as written on whole images. Maps, warps, resamples and stencils publish
//! deferred images that the segment compiler runs as one kernel per segment,
//! and reduce and bins nodes accumulate over those pixels without storing
//! them. The `*Body` statics are what those shapes register. [`Lattice`] and
//! [`Rect`] state where samples sit on the image's full-detail plane.
use crate::{
    Error, Result,
    image::{Image, Raster},
    payload::{DeviceLayout, Persist},
};
use cubecl::prelude::*;
use std::io::{Read, Write};

/// How evaluation runs a node kind. `Kernel`, `Source` and `Foreign` are
/// opaque and receive and produce whole images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The node's function runs on whole materialized inputs whenever a
    /// demanded output is not stored, and each output stays where it wrote it.
    /// With `scaled`, `#[node(scaled)]`, the kernel produces its outputs at
    /// the requested scale itself, as demosaic does. Otherwise it sees scale
    /// 1 and processes the samples it receives.
    Kernel { scaled: bool },
    /// A value bound in memory (`node::source`), handed to each run.
    Source,
    /// The node's host function runs on a thread of its own, on host samples
    /// of its inputs, without a scale, and describes its outputs when it
    /// publishes them. The store keeps the outputs, or the failure, under the
    /// node's key, so the function runs once per key while they stay stored.
    Foreign(Cache),
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
    /// The node's host function returns constants for the body, and its
    /// output is a host value: the body's accumulator over every pixel of its
    /// input, computed in the input's segment without storing it.
    Reduce(&'static ReduceBody),
    /// As `Reduce`, with the output the number of pixels in each bin per
    /// channel.
    Bins(&'static BinsBody),
    /// The node's host function returns a [`Stencil`]: the reach of the body
    /// in full-detail pixels and its constants. The output is a deferred
    /// image whose segment reads the materialized input through a [`Window`]
    /// around each sample, so a window of the output reads that window grown
    /// by the reach.
    Stencil(&'static StencilBody),
    /// The node's host function wires a [`SubGraph`](crate::expand::SubGraph)
    /// of other kinds to its ports, from its parameters and input
    /// descriptions. Evaluation runs the sub-graph's nodes in its place at
    /// the scale the node runs at, keyed like any node's, and the node's
    /// outputs are the sub-graph outputs mapped to its ports.
    Meta,
}
impl Shape {
    /// Whether the kind's contract reads the requested scale to choose its
    /// output lattice: a `scaled` kernel, a warp or a resample. Evaluation
    /// keys such a node by the scale and hands every other node scale 1.
    pub fn scaled(self) -> bool {
        matches!(self, Self::Kernel { scaled: true } | Self::Warp | Self::Resample)
    }

    /// Whether the kind reduces its input to a host value, `Reduce` or
    /// `Bins`. A frontend consumes such a node's outputs.
    pub fn reduces(self) -> bool {
        matches!(self, Self::Reduce(_) | Self::Bins(_))
    }
}

/// Where the store keeps a foreign node's outputs beside memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cache {
    /// In the evaluator's memory only, like computed outputs.
    Memory,
    /// Also on disk, one [`Codec`] per output in port order, so another
    /// evaluator or process reads them instead of running the node.
    Disk(&'static [Codec]),
}

/// How the store writes and reads one output of a [`Persist`] payload.
/// `#[node(foreign = disk)]` generates one per output.
#[derive(Clone, Copy)]
pub struct Codec {
    /// The payload's name. Codecs compare by it.
    pub payload: &'static str,
    pub(crate) write: fn(&Image, &mut dyn Write) -> Result<()>,
    pub(crate) read: fn(&mut dyn Read) -> Result<Image>,
}
impl Codec {
    pub const fn of<P: Persist>() -> Self {
        Self { payload: P::NAME, write: write::<P>, read: read::<P> }
    }
}
fn write<P: Persist>(image: &Image, out: &mut dyn Write) -> Result<()> {
    let raster = image
        .downcast::<P>()
        .ok_or_else(|| Error::Contract(format!("image is not {}", P::NAME)))?;
    P::write(raster.desc(), raster.host()?, out).map_err(|e| Error::Runtime(e.to_string()))
}
/// Fails unless the stream holds a valid description and samples that agree
/// with it.
fn read<P: Persist>(input: &mut dyn Read) -> Result<Image> {
    let (desc, host) = P::read(input).map_err(|e| Error::Runtime(e.to_string()))?;
    P::validate(&desc)?;
    Ok(Raster::<P>::from_host(desc, host)?.into())
}
impl PartialEq for Codec {
    fn eq(&self, other: &Self) -> bool {
        self.payload == other.payload
    }
}
impl Eq for Codec {}
impl std::fmt::Debug for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.payload)
    }
}

/// One sample of a [`MapPayload`] as a map body sees it, channels in storage order.
#[derive(CubeType, Clone, Copy)]
pub struct Pixel {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// A payload the fused kernel reads and writes: packed `[f32; 3]` samples,
/// one [`Pixel`] each, on the [`Lattice`] its description states. Its
/// [`Payload::PLANE`](crate::payload::Payload::PLANE) is
/// [`Plane::of`](crate::image::Plane::of), so evaluation can reduce and
/// window its images without knowing the type.
pub trait MapPayload: DeviceLayout {
    fn lattice(desc: &Self::Desc) -> Lattice;
    /// `desc` with its samples on `lattice`, holding `lattice.window`.
    fn on(desc: &Self::Desc, lattice: Lattice) -> Self::Desc;
}

/// Where stored samples sit. `extent` counts the samples of the whole image
/// per axis, and `full` is the size of the plane they cover in full-detail
/// pixels. Sample `i` sits at plane position `(i + 0.5) · full / extent`
/// along each axis. A raster stores the samples in `window`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lattice {
    pub extent: [u32; 2],
    pub full: [u32; 2],
    pub window: Rect,
}
impl Lattice {
    /// The whole lattice of `extent` samples over a plane of `full` pixels.
    pub fn whole(extent: [u32; 2], full: [u32; 2]) -> Self {
        Self { extent, full, window: Rect { origin: [0; 2], extent } }
    }

    /// This lattice holding `window`, which must lie inside it.
    pub fn with(self, window: Rect) -> Self {
        Self { window, ..self }
    }

    pub fn is_whole(&self) -> bool {
        self.window == Rect { origin: [0; 2], extent: self.extent }
    }

    /// The samples whose cells meet `region`, a rectangle of the plane in
    /// full-detail pixels. `None` when they are none.
    pub fn cover(&self, region: Rect) -> Option<Rect> {
        let axis = |a: usize| {
            let step = f64::from(self.full[a]) / f64::from(self.extent[a]);
            let start = (f64::from(region.origin[a]) / step).floor();
            let end = (f64::from(region.origin[a]) + f64::from(region.extent[a])) / step;
            let clamp = |v: f64| v.clamp(0.0, f64::from(self.extent[a])) as u32;
            (clamp(start), clamp(end.ceil()))
        };
        let ((x0, x1), (y0, y1)) = (axis(0), axis(1));
        (x1 > x0 && y1 > y0).then_some(Rect { origin: [x0, y0], extent: [x1 - x0, y1 - y0] })
    }
}

/// An axis-aligned rectangle of whole cells: `extent` cells per axis from
/// `origin`. A region of a plane counts full-detail pixels, and a window of
/// a lattice counts samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub origin: [u32; 2],
    pub extent: [u32; 2],
}
impl Rect {
    /// One past the last cell per axis.
    pub fn end(&self) -> [u32; 2] {
        [0, 1].map(|a| self.origin[a].saturating_add(self.extent[a]))
    }

    pub fn contains(&self, other: &Rect) -> bool {
        let (end, other_end) = (self.end(), other.end());
        (0..2).all(|a| self.origin[a] <= other.origin[a] && other_end[a] <= end[a])
    }

    pub fn cells(&self) -> usize {
        self.extent[0] as usize * self.extent[1] as usize
    }
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
pub const BOX_SUPPORT: f32 = 0.5;

/// The box filter. Evaluation reduces images finer than the requested scale
/// with it, and `node::filter` offers it as Box.
pub static BOX: FilterBody = FilterBody {
    name: "box",
    support: BOX_SUPPORT,
    interpolating: true,
    expand: box_filter::expand,
};

/// Box: one on `[−½, ½]`. Scaled by a minification footprint it is an area
/// average; both ends are closed, so a sample exactly between two pixels
/// averages them instead of receiving no weight.
#[cube]
pub fn box_filter(x: f32) -> f32 {
    let mut weight = 0.0f32;
    if x.abs() <= 0.5f32 {
        weight = 1.0f32;
    }
    weight
}

/// The expansion of an accumulate body `#[cube] fn(px: Pixel, k: &[f32],
/// acc: &mut [f32])`, which folds one pixel into the accumulator `acc`.
pub type AccumulateExpand = fn(
    &Scope,
    <Pixel as CubeType>::ExpandType,
    &<[f32] as CubeType>::ExpandType,
    &mut <[f32] as CubeType>::ExpandType,
);

/// The expansion of a combine body `#[cube] fn(acc: &mut [f32], other:
/// &[f32])`, which folds the accumulator `other` into `acc`.
pub type CombineExpand =
    fn(&Scope, &mut <[f32] as CubeType>::ExpandType, &<[f32] as CubeType>::ExpandType);

/// A reduction as a reduce node or [`Image::fold`] runs it: a commutative
/// monoid over accumulators of `identity.len()` f32 values. The compiler
/// folds each pixel into a per-lane accumulator, then combines lanes in a
/// fixed order, so equal inputs give equal bits on one runtime.
#[derive(Clone, Copy)]
pub struct ReduceBody {
    /// Unique among reduce bodies. Kernel identity compares bodies by name.
    pub name: &'static str,
    /// How many constants `accumulate` reads from `k`.
    pub constants: usize,
    /// The accumulator before any pixel. `combine` with it changes nothing.
    pub identity: &'static [f32],
    pub accumulate: AccumulateExpand,
    /// Associative and commutative: lanes combine in an order the compiler
    /// chooses.
    pub combine: CombineExpand,
}

/// Where a pixel sits in the window a bins pass covers: its offset from the
/// window's first sample and the window's extent, in samples.
#[derive(CubeType, Clone, Copy)]
pub struct Position {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The bin one pixel counts in for each channel of the counts, `c0` first.
/// A body with fewer than three channels leaves the rest unread. An index of
/// [`BinsBody::bins`] or more counts nothing.
#[derive(CubeType, Clone, Copy)]
pub struct Bin {
    pub c0: u32,
    pub c1: u32,
    pub c2: u32,
}

/// The expansion of a bin body `#[cube] fn(px: Pixel, at: Position, k:
/// &[f32]) -> Bin`.
pub type BinsExpand = fn(
    &Scope,
    <Pixel as CubeType>::ExpandType,
    <Position as CubeType>::ExpandType,
    &<[f32] as CubeType>::ExpandType,
) -> <Bin as CubeType>::ExpandType;

/// A histogram as a bins node or [`Image::count`] runs it: `bins` u32 counts
/// for each of `channels` channels, 1 to 3, stored bin-major (count `i` of
/// channel `c` at `i · channels + c`). Integer sums are exact, so the counts
/// are independent of the order the compiler merges them in.
#[derive(Clone, Copy)]
pub struct BinsBody {
    /// Unique among bins bodies. Kernel identity compares bodies by name.
    pub name: &'static str,
    /// How many constants `bin` reads from `k`.
    pub constants: usize,
    pub bins: usize,
    pub channels: usize,
    pub bin: BinsExpand,
}

/// What a stencil's host function returns: how far the body reads, in
/// full-detail pixels of the input's plane, and the `N` constants the body
/// reads as `k[0..N]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stencil<const N: usize> {
    /// Finite and not negative. The compiler converts it to samples of the
    /// input's lattice ([`Window::radius_x`]).
    pub radius: f32,
    pub constants: [f32; N],
}

/// What [`Window::at`] returns for a position outside the input's lattice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Border {
    /// The nearest edge sample.
    Repeat,
    /// Black, zero in every channel.
    Zero,
}

/// The neighbourhood of one output sample as a stencil body reads it, on the
/// lattice of the stencil's input, which is the output's.
///
/// `radius_x` and `radius_y` are the host function's radius converted to
/// samples of this lattice, `ceil(radius · extent / full)` and at least one,
/// and `rate_x` and `rate_y` are samples per full-detail pixel,
/// `extent / full`. A body states lengths in full-detail pixels and scales
/// them by the rate, so a preview computes the same image at its detail.
#[derive(CubeType)]
pub struct Window<'a> {
    pub(crate) input: &'a [f32],
    /// The output sample's position in the lattice.
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// The stored window: its first sample and its width, the row stride.
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) stride: i32,
    /// The lattice's extent.
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub radius_x: i32,
    pub radius_y: i32,
    pub rate_x: f32,
    pub rate_y: f32,
    #[cube(comptime)]
    pub(crate) border: Border,
}

#[cube]
impl<'a> Window<'a> {
    /// The input sample `dx` columns right of and `dy` rows below the output
    /// sample's position, both at most the radius on their axis. A position
    /// outside the lattice follows the body's [`Border`].
    pub fn at(&self, dx: i32, dy: i32) -> Pixel {
        let mut u = self.x + dx;
        let mut v = self.y + dy;
        let mut inside = true;
        if comptime!(self.border == Border::Repeat) {
            u = i32::clamp(u, 0i32, self.width - 1i32);
            v = i32::clamp(v, 0i32, self.height - 1i32);
        } else {
            inside = u >= 0i32 && u < self.width && v >= 0i32 && v < self.height;
        }
        let mut r = 0.0f32;
        let mut g = 0.0f32;
        let mut b = 0.0f32;
        // The stored window holds the reach of every output sample it
        // serves, clamped to the lattice (`Plan::input_window`).
        if inside {
            let i = usize::cast_from((v - self.top) * self.stride + u - self.left) * 3;
            r = self.input[i];
            g = self.input[i + 1];
            b = self.input[i + 2];
        }
        Pixel { r, g, b }
    }
}

/// The expansion of a stencil body `#[cube] fn(w: &Window, k: &[f32]) ->
/// Pixel`, the output sample computed from the window around it.
pub type StencilExpand = for<'a> fn(
    &Scope,
    &<Window<'a> as CubeType>::ExpandType,
    &<[f32] as CubeType>::ExpandType,
) -> <Pixel as CubeType>::ExpandType;

/// A stencil as a stencil node or [`Image::stencil`] runs it. Kernel
/// identity compares bodies by name, so each body compiles once per
/// sequence of maps after it, whatever its radius and constants.
#[derive(Clone, Copy)]
pub struct StencilBody {
    /// Unique among stencil bodies.
    pub name: &'static str,
    /// How many constants `expand` reads from `k`.
    pub constants: usize,
    pub border: Border,
    pub expand: StencilExpand,
}

/// Equality, hashing and debug output by `name`, which kernel identity
/// compares.
macro_rules! by_name {
    ($($body:ident),*) => {$(
        impl PartialEq for $body {
            fn eq(&self, other: &Self) -> bool {
                self.name == other.name
            }
        }
        impl Eq for $body {}
        impl std::hash::Hash for $body {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.name.hash(state);
            }
        }
        impl std::fmt::Debug for $body {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.name)
            }
        }
    )*};
}
by_name!(FilterBody, ReduceBody, BinsBody, StencilBody);
