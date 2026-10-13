//! What the deferred shapes hand the segment compiler: the `#[cube]` bodies
//! of maps, reductions and stencils, resampling filters and warps, and the
//! [`Pixel`] they read. [`matrix`] holds the 3×3 math that warps and colour
//! transforms share.
//!
//! A map, reduce, bins or stencil kind registers a `static` body, and its
//! host function returns the body's constants. The compiler (`plan.rs`)
//! inlines the bodies of a segment into one kernel, so kernel identity
//! compares bodies by name and each sequence compiles once whatever the
//! constants.
use crate::{
    lattice::Lattice,
    payload::{DeviceLayout, HostBuffer},
};
use cubecl::prelude::*;

/// One sample of a [`MapPayload`] as a body sees it. This is the layout
/// every kernel of the segment compiler reads and writes: three f32
/// channels per sample, packed, in storage order.
#[derive(CubeType, Clone, Copy)]
pub struct Pixel {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// A payload the fused kernel reads and writes: one [`Pixel`] per sample,
/// on the [`Lattice`] its description states. The host bound makes the
/// layout part of the type, so a payload of another channel count cannot
/// claim it. Its [`Payload::PLANE`](crate::payload::Payload::PLANE) is
/// [`Plane::of`](crate::image::Plane::of), so evaluation can reduce and
/// window its images without knowing the type.
pub trait MapPayload: DeviceLayout<Host = HostBuffer<[f32; 3]>> {
    fn lattice(desc: &Self::Desc) -> Lattice;
    /// `desc` with its samples on `lattice`, holding `lattice.window`.
    fn on(desc: &Self::Desc, lattice: Lattice) -> Self::Desc;
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
pub type Projective = matrix::Mat3;

/// 3×3 matrices in f64, row-major, for warps and colour transforms alike.
pub mod matrix {
    pub type Mat3 = [[f64; 3]; 3];

    pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
    }

    pub fn apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
        m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
    }

    /// The point `m` takes `[x, y]` to, `None` where its homogeneous `w` is
    /// not positive, beyond the map's horizon.
    pub fn project(m: &Mat3, [x, y]: [f64; 2]) -> Option<[f64; 2]> {
        let [x, y, w] = apply(m, [x, y, 1.0]);
        (w > 0.0).then(|| [x / w, y / w])
    }

    pub fn transpose(m: Mat3) -> Mat3 {
        std::array::from_fn(|i| std::array::from_fn(|j| m[j][i]))
    }

    fn cofactor(m: &Mat3, i: usize, j: usize) -> f64 {
        let (r0, r1, c0, c1) = ((i + 1) % 3, (i + 2) % 3, (j + 1) % 3, (j + 2) % 3);
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    }

    pub fn determinant(m: &Mat3) -> f64 {
        (0..3).map(|j| m[0][j] * cofactor(m, 0, j)).sum()
    }

    /// Inverse by the adjugate. Callers invert only well-conditioned matrices.
    pub fn inverse(m: &Mat3) -> Mat3 {
        let det = determinant(m);
        std::array::from_fn(|i| std::array::from_fn(|j| cofactor(m, j, i) / det))
    }

    pub fn to_f32(m: &Mat3) -> [[f32; 3]; 3] {
        m.map(|row| row.map(|v| v as f32))
    }

    pub fn to_f64(m: &[[f32; 3]; 3]) -> Mat3 {
        m.map(|row| row.map(f64::from))
    }
}

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

/// A reduction as a reduce node or [`Image::fold`](crate::image::Image::fold) runs it: a commutative
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

/// A histogram as a bins node or [`Image::count`](crate::image::Image::count) runs it: `bins` u32 counts
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

/// A stencil as a stencil node or [`Image::stencil`](crate::image::Image::stencil) runs it. Kernel
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
