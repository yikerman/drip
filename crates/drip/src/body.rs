//! What the deferred shapes hand the segment compiler: the `#[cube]` bodies
//! of maps, reductions and stencils, resampling filters, warps with their
//! optional [`Radial`] part, and the [`Pixel`] they read. [`matrix`] holds the
//! 3×3 math that warps and colour transforms share.
//!
//! A map, reduce, bins or stencil kind registers a `static` body, and its
//! host function returns the body's constants. The compiler (`plan.rs`)
//! inlines the bodies of a segment into one kernel, so kernel identity
//! compares bodies by name and each sequence compiles once whatever the
//! constants.
use crate::{
    Error, Result,
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
    /// Kernel identity compares bodies by name.
    pub name: &'static str,
    pub constants: usize,
    pub expand: Expand,
}

/// The constants a map's host function returns: `[f32; N]`, read by the body
/// as `k[0..N]`.
pub trait Constants {
    const LEN: usize;
}
impl<const N: usize> Constants for [f32; N] {
    const LEN: usize = N;
}

/// A step a map, warp, resample or stencil node adds to its input, which
/// [`Image::step`](crate::image::Image::step) composes into the input's plan.
#[derive(Clone, Copy, Debug)]
pub enum Step<'a> {
    /// A map body and the constants it reads.
    Map(&'a MapBody, &'a [f32]),
    Warp(&'a Warp),
    /// Resampling with a filter onto the lattice the step's description
    /// states, which covers the same image.
    Resample(&'static FilterBody),
    /// A stencil body, how far it reads in full-detail pixels, and the
    /// constants it reads.
    Stencil(&'a StencilBody, f32, &'a [f32]),
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
/// (`[0, full]` along each axis). With a `radial` part they take the output
/// plane to the radial part's output side, which shares the input plane's
/// coordinates, and the radial part takes each channel on to the input:
///
/// ```text
/// output point ─► maps[c] ─► q ─► radial, channel c ─► input point, gain
/// ```
///
/// Output points whose homogeneous `w` is not positive, or that land outside
/// the input plane, are black.
#[derive(Clone, Debug, PartialEq)]
pub struct Warp {
    pub maps: [Projective; 3],
    /// The filter that reconstructs the input where the maps move samples
    /// off the input lattice and no Resample chooses one. `None` declares a
    /// reindex: every output sample lands on an input sample, which
    /// evaluation checks. A warp with a radial part always filters.
    pub filter: Option<&'static FilterBody>,
    pub radial: Option<Radial>,
}
impl Warp {
    /// One map for every channel that moves samples onto samples only, such
    /// as a reflection or quarter turn of the whole plane.
    pub fn reindex(map: Projective) -> Self {
        Self { maps: [map; 3], filter: None, radial: None }
    }
    /// Maps that may move samples between lattice positions, reconstructed
    /// with `filter`.
    pub fn filtered(maps: [Projective; 3], filter: &'static FilterBody) -> Self {
        Self { maps, filter: Some(filter), radial: None }
    }
    /// `maps` followed by `radial`, reconstructed with `filter`.
    pub fn radial(maps: [Projective; 3], radial: Radial, filter: &'static FilterBody) -> Self {
        Self { maps, filter: Some(filter), radial: Some(radial) }
    }
}

/// A radial map of the warp's input plane about `centre`, the form lens
/// distortion, lateral chromatic aberration and vignetting take. With
/// `ρ = |q − centre| / radius`, channel `c` of a point `q` reads
///
/// ```text
/// p_c = q + (s_c(ρ) − 1) · (q − centre),   s_c = ratios[c]
/// ```
///
/// so `s_c` is the ratio of the source's distance from the centre to `q`'s.
/// The value reconstructed at `p_c` is multiplied by `gain(ρ_c)`, where
/// `ρ_c = |s_c(ρ)| · ρ` is the source's radius. The gain is defined on the
/// input, which a map body cannot express: map bodies see no position.
/// Lengths are full-detail pixels of the input plane, so the map is the same
/// at every lattice of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Radial {
    pub centre: [f64; 2],
    /// Positive: the distance from the centre where `ρ = 1`.
    pub radius: f64,
    pub ratios: [Spline; 3],
    /// `None` multiplies by one, and the kernel skips the gain.
    pub gain: Option<Spline>,
}
impl Radial {
    /// The point channel `c` reads for `q`.
    pub fn source(&self, c: usize, q: [f64; 2]) -> [f64; 2] {
        let s = self.ratios[c].at(self.rho(q));
        [0, 1].map(|a| q[a] + (s - 1.0) * (q[a] - self.centre[a]))
    }

    /// The factor channel `c`'s value at `q` is multiplied by.
    pub fn gain(&self, c: usize, q: [f64; 2]) -> f64 {
        let rho = self.rho(q);
        self.gain.as_ref().map_or(1.0, |gain| gain.at(self.ratios[c].at(rho).abs() * rho))
    }

    /// A box holding the points channel `c` reads for the points of `area`,
    /// a box given by its least and greatest corner.
    ///
    /// Per axis, `p_c − centre = s · (q − centre)` with `s` within the
    /// ratio's range over the box's radii and `q − centre` within the box,
    /// so the products of the interval ends bound it.
    pub fn sources(&self, c: usize, area: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
        let [lo, hi] = area;
        let radii = self.radii(area);
        let ratios = self.ratios[c].range(radii[0], radii[1]);
        let axis = |a: usize| {
            let offsets = [lo[a], hi[a]].map(|v| v - self.centre[a]);
            let products = ratios.iter().flat_map(|s| offsets.map(|d| s * d));
            let (min, max) = products
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), v| (min.min(v), max.max(v)));
            [min + self.centre[a], max + self.centre[a]]
        };
        let ([x0, x1], [y0, y1]) = (axis(0), axis(1));
        [[x0, y0], [x1, y1]]
    }

    /// A bound on how far channel `c` stretches distances inside `area`: the
    /// operator norm of the map's Jacobian. The Jacobian is
    /// `s·I + s′ρ · r̂r̂ᵀ` with `r̂` the unit vector from the centre, whose
    /// eigenvalues are `s` across the radius and `s + s′ρ` along it.
    pub fn stretch(&self, c: usize, area: [[f64; 2]; 2]) -> f64 {
        let [near, far] = self.radii(area);
        let [low, high] = self.ratios[c].range(near, far);
        low.abs().max(high.abs()) + self.ratios[c].steepest(near, far) * far
    }

    fn rho(&self, q: [f64; 2]) -> f64 {
        (q[0] - self.centre[0]).hypot(q[1] - self.centre[1]) / self.radius
    }

    /// The least and greatest `ρ` of the points of `area`: the centre held
    /// to the box, and the farthest corner.
    fn radii(&self, [lo, hi]: [[f64; 2]; 2]) -> [f64; 2] {
        let near = [0, 1].map(|a| self.centre[a].max(lo[a]).min(hi[a]) - self.centre[a]);
        let far =
            [0, 1].map(|a| (lo[a] - self.centre[a]).abs().max((hi[a] - self.centre[a]).abs()));
        [near[0].hypot(near[1]) / self.radius, far[0].hypot(far[1]) / self.radius]
    }
}

/// A piecewise-linear function on uniform knots: `values[i]` at
/// `start + i · step`, linear between knots, and the first and last value
/// held beyond them. The fused kernel evaluates it the same way in f32.
#[derive(Clone, Debug, PartialEq)]
pub struct Spline {
    start: f64,
    step: f64,
    values: Vec<f64>,
}
impl Spline {
    /// Knots a spline holds at most. The fused kernel reserves constants for
    /// this many per spline, so a spline's length stays out of kernel
    /// identity.
    pub const MAX_KNOTS: usize = 256;

    /// Fails unless `start` is finite, `step` is finite and positive, and
    /// `values` holds 2 to [`MAX_KNOTS`](Self::MAX_KNOTS) finite values.
    pub fn new(start: f64, step: f64, values: Vec<f64>) -> Result<Self> {
        let finite = start.is_finite() && step.is_finite() && values.iter().all(|v| v.is_finite());
        if !(finite && step > 0.0 && (2..=Self::MAX_KNOTS).contains(&values.len())) {
            let message =
                format!("a spline needs a positive step and 2 to {} finite knots", Self::MAX_KNOTS);
            return Err(Error::Contract(message));
        }
        Ok(Self { start, step, values })
    }

    /// `value` everywhere. Fails when it is not finite.
    pub fn constant(value: f64) -> Result<Self> {
        Self::new(0.0, 1.0, vec![value; 2])
    }

    pub fn start(&self) -> f64 {
        self.start
    }

    pub fn step(&self) -> f64 {
        self.step
    }

    pub fn values(&self) -> &[f64] {
        &self.values
    }

    pub fn at(&self, x: f64) -> f64 {
        let last = (self.values.len() - 1) as f64;
        let t = ((x - self.start) / self.step).clamp(0.0, last);
        let i = (t as usize).min(self.values.len() - 2);
        self.values[i] + (t - i as f64) * (self.values[i + 1] - self.values[i])
    }

    /// The least and greatest value over `[lo, hi]`: the ends and the knots
    /// between them.
    pub fn range(&self, lo: f64, hi: f64) -> [f64; 2] {
        let knots = (0..self.values.len()).filter(|&i| (lo..=hi).contains(&self.knot(i)));
        let values = [self.at(lo), self.at(hi)].into_iter().chain(knots.map(|i| self.values[i]));
        let (min, max) = values
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), v| (min.min(v), max.max(v)));
        [min, max]
    }

    /// The greatest absolute slope over `[lo, hi]`, zero beyond the knots.
    pub fn steepest(&self, lo: f64, hi: f64) -> f64 {
        (0..self.values.len() - 1)
            .filter(|&i| self.knot(i) <= hi && self.knot(i + 1) >= lo)
            .map(|i| (self.values[i + 1] - self.values[i]).abs() / self.step)
            .fold(0.0, f64::max)
    }

    fn knot(&self, i: usize) -> f64 {
        self.start + i as f64 * self.step
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

/// A stencil as a stencil node or [`Step::Stencil`] runs it. Kernel
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
by_name!(MapBody, FilterBody, ReduceBody, BinsBody, StencilBody);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spline_interpolates_its_knots_and_holds_its_ends() {
        // Knots 1, 2, 0 at 0.5, 0.75, 1.
        let s = Spline::new(0.5, 0.25, vec![1.0, 2.0, 0.0]).unwrap();
        let at = [0.0, 0.5, 0.625, 0.875, 2.0].map(|x| s.at(x));
        assert_eq!(at, [1.0, 1.0, 1.5, 1.0, 0.0]);
        assert_eq!(s.range(0.6, 0.9), [s.at(0.9), 2.0]);
        assert_eq!(
            [s.steepest(0.6, 0.9), s.steepest(0.0, 0.4), s.steepest(1.1, 2.0)],
            [8.0, 0.0, 0.0]
        );
        assert!(Spline::new(0.0, 1.0, vec![1.0]).is_err());
        assert!(Spline::new(0.0, 0.0, vec![1.0, 1.0]).is_err());
        assert!(Spline::constant(f64::NAN).is_err());
    }

    /// Sources and numerical Jacobians over a grid of a box stay inside the
    /// box `sources` returns and below `stretch`.
    #[test]
    fn radial_bounds_hold_the_box_they_describe() {
        let ratio = Spline::new(0.1, 0.3, vec![1.1, 0.9, 1.05, 1.3]).unwrap();
        let radial = Radial {
            centre: [40.0, 30.0],
            radius: 50.0,
            ratios: [ratio.clone(), ratio.clone(), ratio],
            gain: None,
        };
        for area in [[[35.0, 20.0], [60.0, 33.0]], [[0.0, 0.0], [12.0, 9.0]]] {
            let [lo, hi] = radial.sources(0, area);
            let stretch = radial.stretch(0, area);
            for i in 0..=20 {
                for j in 0..=20 {
                    let t = [i, j].map(|v| f64::from(v) / 20.0);
                    let q = [0, 1].map(|a| area[0][a] + t[a] * (area[1][a] - area[0][a]));
                    let p = radial.source(0, q);
                    let inside = (0..2).all(|a| lo[a] - 1e-9 <= p[a] && p[a] <= hi[a] + 1e-9);
                    assert!(inside, "{q:?} → {p:?}");
                    let e = 1e-6;
                    let dx = radial.source(0, [q[0] + e, q[1]]);
                    let dy = radial.source(0, [q[0], q[1] + e]);
                    // Columns of the Jacobian bound its operator norm from below.
                    for d in [dx, dy] {
                        let length = (d[0] - p[0]).hypot(d[1] - p[1]) / e;
                        assert!(length <= stretch + 1e-6, "{q:?}: {length} > {stretch}");
                    }
                }
            }
        }
    }
}
