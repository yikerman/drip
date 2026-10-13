//! The segment compiler: the deferred steps of maps, warps, resamples and
//! stencils run as one kernel.
//!
//! A [`Plan`] holds one stage, resampling or a stencil, then map bodies. A
//! resampling stage holds per-channel projective maps from the image's plane
//! to its input's plane with their filter. A stencil stage holds a
//! [`StencilBody`] over the input's own lattice. Every kernel of a plan
//! evaluates a pixel the same way, so intermediate images never reach memory:
//!
//! ```text
//! input raster ─► stage: load | gather | filter | stencil ─► body ─► … ─► body ─┬─► store
//!                                                                              └─► fold, count
//! ```
//!
//! # Composition
//!
//! A step composes into a plan only where the result equals running the
//! nodes one by one:
//!
//! - A reindex (Orientation, an integer crop) commutes with per-pixel maps
//!   and joins the stage wherever it sits.
//! - A filtering warp or a Resample before any map composes into the stage.
//!   After a map it cuts the segment: the maps materialize on their own
//!   lattice, then the resampling runs as its own launch. A weighted average
//!   of sigmoid values is not the sigmoid of the average.
//! - A Resample fixes the stage's filter and lattice, so a filtering step
//!   after it resamples again.
//! - A stencil starts a plan over its materialized input, and any warp or
//!   Resample after it starts another. Maps after it join it.
//! - Steps after a fan-out each extend the shared plan and recompute it in
//!   their own kernel, which trades arithmetic for bandwidth.
//!
//! At run time the stage loads samples when the composed map is the identity
//! on an equal lattice and the filter interpolates, gathers them when only
//! reindexes composed, and filters otherwise. A plan that neither moves nor
//! maps samples launches nothing and shares its input. Kernel identity is the
//! stage and the body sequence, so each combination compiles once whatever
//! the constants.
//!
//! # Windows
//!
//! A plan computes the window of its output lattice that a consumer asks
//! for, and [`Plan::input_window`] bounds what that window reads: the same
//! window through a load, the reindexed corners through a gather, through a
//! filter the mapped corners grown by the support times the footprint, and
//! through a stencil the window grown by its reach. A projective map keeps
//! lines straight, so the corners bound the window's image. The kernel
//! positions samples by whole-lattice index and clamps taps to the stored
//! window, so a window's samples equal the same samples of the whole image
//! bit for bit. Tiled export relies on this.
//!
//! # Reductions
//!
//! - A fold gives each lane at most [`FAN_IN`] pixels, strided across the
//!   window, then combines at most `FAN_IN` accumulators per lane per pass
//!   until one remains. The order is fixed and no float atomics run, so
//!   equal inputs give equal bits on one runtime.
//! - A count of up to [`SHARED_COUNTS`] bins accumulates per workgroup in
//!   shared memory, and a merge pass sums the workgroups' copies. Larger
//!   counts go to at most [`SHARDS`] copies in global memory, each lane
//!   adding a run of its pixels in one bin at once.
//! - Kernels with barriers take at most the runtime's lanes per workgroup,
//!   since the CPU runtime runs a host worker per lane.
use crate::{
    Error, Result,
    body::{
        Bin, BinsBody, FilterBody, MapBody, MapPayload, Pixel, Position, Projective, ReduceBody,
        StencilBody, Warp, Window,
    },
    image::Raster,
    lattice::{Lattice, Rect},
    runtime::{KernelContext, dispatch_dims},
};
use cubecl::prelude::*;
use std::sync::Arc;

/// Footprints wider than this many source samples per output sample are
/// clamped, bounding taps per output sample. Stronger reductions alias. A
/// prefiltered pyramid or EWA would lift the bound.
pub const MAX_FOOTPRINT: f32 = 16.0;

const IDENTITY: Projective = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Values one lane of a fold or combine pass reads, so a pass takes the
/// count down by this factor.
const FAN_IN: usize = 256;
/// Bin counts per workgroup that fit in shared memory: 16 KiB of u32.
const SHARED_COUNTS: usize = 4096;
/// Pixels each workgroup counts into shared memory, at least. Each
/// workgroup's counts are one more copy for the merge to read.
const GROUP_PIXELS: usize = 4096;
/// Copies of counts too large for shared memory, at most.
const SHARDS: usize = 32;
/// Counts all copies hold together, at most: 4 MiB, within the L2 cache of
/// current GPUs, where scattered atomics stay cheap. A level-1 waveform of
/// the Sony fixture, 196 608 counts, takes 6.1 ms on an RTX 3080 in 32
/// copies and 2.0 ms in five.
const SHARD_COUNTS: usize = 1 << 20;

/// A stage, resampling or a stencil, followed by map bodies, with the
/// stencil's and the bodies' constants concatenated.
#[derive(Clone)]
pub(crate) struct Plan {
    /// Per channel, the image's plane to the input raster's plane.
    maps: [Projective; 3],
    /// The filter where `maps` move samples off the input lattice. `None`
    /// while every warp so far was a reindex.
    filter: Option<&'static FilterBody>,
    /// A Resample fixed the stage's filter and lattice.
    resampled: bool,
    /// The stage is this stencil over the input's lattice, with its radius
    /// in full-detail pixels, and `maps` are the identity.
    stencil: Option<(StencilBody, f32)>,
    bodies: Vec<MapBody>,
    constants: Vec<f32>,
}
impl Default for Plan {
    fn default() -> Self {
        Self {
            maps: [IDENTITY; 3],
            filter: None,
            resampled: false,
            stencil: None,
            bodies: Vec::new(),
            constants: Vec::new(),
        }
    }
}
impl Plan {
    /// `body` with `radius` and `constants` as the stage of a new plan.
    /// `None` unless this plan is empty: a stencil reads materialized
    /// samples, so it starts a segment.
    pub fn stencil(&self, body: &StencilBody, radius: f32, constants: &[f32]) -> Option<Self> {
        let empty = self.maps == [IDENTITY; 3] && self.filter.is_none() && self.open();
        empty.then(|| Self {
            stencil: Some((*body, radius)),
            constants: constants.to_vec(),
            ..Self::default()
        })
    }

    /// This plan followed by `body`. `constants` holds `body.constants` values.
    pub fn then(&self, body: &MapBody, constants: &[f32]) -> Self {
        let mut plan = self.clone();
        plan.bodies.push(*body);
        plan.constants.extend_from_slice(constants);
        plan
    }

    /// This plan followed by `warp`, which [`reindexes`] or not. `None` when
    /// the warp resamples this plan's output instead.
    pub fn warp(&self, warp: &Warp, reindex: bool) -> Option<Self> {
        // A reindex filters nothing, so the stage keeps its filter.
        let filter = if reindex { self.filter } else { warp.filter };
        let composes = reindex && self.stencil.is_none() || self.open();
        composes.then(|| Self { maps: compose(&self.maps, &warp.maps), filter, ..self.clone() })
    }

    /// This plan resampled with `filter` from its output lattice `from` onto
    /// `to`, which covers the same image. `None` when the resample runs on
    /// this plan's output instead.
    pub fn resample(
        &self,
        filter: &'static FilterBody,
        from: Lattice,
        to: Lattice,
    ) -> Option<Self> {
        let scale = |axis: usize| f64::from(from.full[axis]) / f64::from(to.full[axis]);
        let map = [[scale(0), 0.0, 0.0], [0.0, scale(1), 0.0], [0.0, 0.0, 1.0]];
        self.open().then(|| Self {
            maps: compose(&self.maps, &[map; 3]),
            filter: Some(filter),
            resampled: true,
            ..self.clone()
        })
    }

    /// The stencil's reach on its input lattice `from`, if the stage is one.
    fn reach(&self, from: Lattice) -> Option<Reach> {
        self.stencil.map(|(_, radius)| Reach::of(radius, from))
    }

    /// No body has run, no Resample fixed the stage and the stage is no
    /// stencil, so a filtering step still composes into it.
    fn open(&self) -> bool {
        !self.resampled && self.bodies.is_empty() && self.stencil.is_none()
    }

    /// Run the plan on `input` into a new raster described by `desc`, which
    /// may hold a window of its lattice. `input` holds at least
    /// [`input_window`](Self::input_window) of its own. A plan that neither
    /// moves nor maps samples shares the input's samples when the windows
    /// agree.
    pub fn run<P: MapPayload>(
        &self,
        input: &Raster<P>,
        desc: Arc<P::Desc>,
        ctx: &KernelContext<'_>,
    ) -> Result<Raster<P>> {
        let (from, to) = (P::lattice(input.desc()), P::lattice(&desc));
        let (kernel, constants) = self.compile(from, to, ctx)?;
        if kernel.stage == Stage::Load && self.bodies.is_empty() && from.window == to.window {
            return input.share_as(desc);
        }
        let constants = ctx.constants(&constants)?;
        let output = Raster::from_device(desc.clone(), ctx.device_buffer::<P>(&desc)?, ctx)?;
        let (count, dim) = dispatch_dims(to.window.cells());
        kernel::fused::launch(
            ctx.client()?,
            count,
            dim,
            input.device(ctx)?.argument(),
            constants.argument(),
            output.device(ctx)?.argument(),
            stored(from),
            domain(to),
            kernel,
        );
        ctx.runtime.count_fused(to.window.cells());
        Ok(output)
    }

    /// Fold the plan's output over the window of lattice `to` with `body`,
    /// from `input`, which holds [`input_window`](Self::input_window). Returns
    /// the accumulator. `constants` holds `body.constants` values.
    pub fn fold<P: MapPayload>(
        &self,
        input: &Raster<P>,
        to: Lattice,
        body: &ReduceBody,
        constants: &[f32],
        ctx: &KernelContext<'_>,
    ) -> Result<Vec<f32>> {
        let from = P::lattice(input.desc());
        let (kernel, mut k) = self.compile(from, to, ctx)?;
        k.extend_from_slice(body.identity);
        k.extend_from_slice(constants);
        let client = ctx.client()?;
        let width = body.identity.len();
        // Each lane folds at most `FAN_IN` pixels, then each pass combines
        // at most `FAN_IN` accumulators per lane, until one remains.
        let mut len = to.window.cells().div_ceil(FAN_IN);
        let mut partial = ctx.scratch_32(len * width)?;
        let dim = lanes(client);
        kernel::fold::launch(
            client,
            grid(len, dim),
            dim,
            input.device(ctx)?.argument(),
            ctx.constants(&k)?.argument(),
            partial.argument(),
            stored(from),
            domain(to),
            kernel,
            *body,
        );
        ctx.runtime.count_reduction();
        while len > 1 {
            let next = len.div_ceil(FAN_IN);
            let output = ctx.scratch_32(next * width)?;
            kernel::combine::launch(
                client,
                grid(next, dim),
                dim,
                partial.argument(),
                output.argument(),
                *body,
            );
            (partial, len) = (output, next);
        }
        let bytes = ctx.runtime.read(partial.handle)?;
        Ok(bytes.as_chunks::<4>().0.iter().map(|&v| f32::from_ne_bytes(v)).collect())
    }

    /// Count the plan's output over the window of lattice `to` into the bins
    /// of `body`, from `input`, which holds
    /// [`input_window`](Self::input_window). Returns `body.bins ·
    /// body.channels` counts, bin-major. `constants` holds `body.constants`
    /// values.
    pub fn count<P: MapPayload>(
        &self,
        input: &Raster<P>,
        to: Lattice,
        body: &BinsBody,
        constants: &[f32],
        ctx: &KernelContext<'_>,
    ) -> Result<Vec<u32>> {
        if !(1..=3).contains(&body.channels) || body.bins == 0 {
            return Err(Error::Contract(format!("{} needs bins and 1 to 3 channels", body.name)));
        }
        let from = P::lattice(input.desc());
        let (kernel, mut k) = self.compile(from, to, ctx)?;
        k.extend_from_slice(constants);
        let client = ctx.client()?;
        let (input, k) = (input.device(ctx)?.argument(), ctx.constants(&k)?);
        let len = body.bins * body.channels;
        let dim = lanes(client);
        // Counts that fit in shared memory accumulate per workgroup without
        // global atomics. Larger ones go to copies in global memory, so lanes
        // counting the same bin, such as neutral pixels in a vectorscope,
        // rarely touch the same address.
        let (parts, partial) = if len <= SHARED_COUNTS {
            let groups = to.window.cells().div_ceil(GROUP_PIXELS).clamp(1, 256);
            let partial = ctx.scratch_32(groups * len)?;
            kernel::count_groups::launch(
                client,
                CubeCount::Static(groups as u32, 1, 1),
                dim,
                input,
                k.argument(),
                partial.argument(),
                stored(from),
                domain(to),
                kernel,
                *body,
            );
            (groups, partial)
        } else {
            let shards = (SHARD_COUNTS / len).clamp(1, SHARDS);
            let partial = ctx.scratch_32(shards * len)?;
            kernel::clear::launch(client, grid(shards * len, dim), dim, partial.argument());
            kernel::count_shards::launch(
                client,
                CubeCount::Static(256, 1, 1),
                dim,
                input,
                k.argument(),
                partial.argument(),
                stored(from),
                domain(to),
                shards,
                kernel,
                *body,
            );
            (shards, partial)
        };
        ctx.runtime.count_reduction();
        let output = ctx.scratch_32(len)?;
        kernel::merge::launch(
            client,
            grid(len, dim),
            dim,
            partial.argument(),
            output.argument(),
            parts,
        );
        let bytes = ctx.runtime.read(output.handle)?;
        Ok(bytes.as_chunks::<4>().0.iter().map(|&v| u32::from_ne_bytes(v)).collect())
    }

    /// The kernel identity and constants that compute the window of lattice
    /// `to` from lattice `from`: the stage's index maps, unless it loads, or
    /// a stencil's reach and rate per axis, then the stencil's and the
    /// bodies' constants. Counts a stencil that reaches less than one sample
    /// of a preview lattice on `ctx`'s runtime.
    fn compile(
        &self,
        from: Lattice,
        to: Lattice,
        ctx: &KernelContext<'_>,
    ) -> Result<(Kernel, Vec<f32>)> {
        let (stage, maps) = self.stage(from, to)?;
        let mut constants: Vec<f32> = match (stage, self.reach(from)) {
            (_, Some(reach)) => {
                if reach.approximates {
                    ctx.runtime.count_approximated();
                }
                let [rx, ry] = reach.samples.map(|r| r as f32);
                vec![rx, ry, reach.rate[0], reach.rate[1]]
            }
            (Stage::Load | Stage::Stencil(_), None) => Vec::new(),
            (Stage::Gather | Stage::Filter(_), None) => {
                maps.iter().flat_map(|m| m.as_flattened()).map(|&v| v as f32).collect()
            }
        };
        if constants.iter().any(|v| !v.is_finite()) {
            return Err(Error::Contract("warp maps do not fit f32".into()));
        }
        constants.extend_from_slice(&self.constants);
        Ok((Kernel { stage, bodies: self.bodies.clone() }, constants))
    }

    /// The window of the input lattice `from` that computing the window of
    /// the output lattice `to` reads, clamped to the input. Where the maps
    /// take a corner beyond the horizon, the whole input. A stencil reads
    /// the window grown by its reach.
    pub fn input_window(&self, from: Lattice, to: Lattice) -> Result<Rect> {
        if let Some(reach) = self.reach(from) {
            return Ok(reach.grow(to.window, from.extent));
        }
        let (stage, maps) = self.stage(from, to)?;
        let support = match stage {
            Stage::Load => return Ok(to.window),
            Stage::Gather | Stage::Stencil(_) => None,
            Stage::Filter(filter) => Some(filter.support),
        };
        let whole = Rect { origin: [0; 2], extent: from.extent };
        let [x0, y0] = to.window.origin.map(f64::from);
        let [x1, y1] = to.window.end().map(|end| f64::from(end) - 1.0);
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for m in &maps {
            for [x, y] in [[x0, y0], [x1, y0], [x0, y1], [x1, y1]] {
                let w = m[2][0] * x + m[2][1] * y + m[2][2];
                // Also a NaN `w`, which no comparison orders.
                if w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                    return Ok(whole);
                }
                for axis in 0..2 {
                    let at = (m[axis][0] * x + m[axis][1] * y + m[axis][2]) / w;
                    // The kernel's taps reach `support · footprint` samples
                    // from a position, and its f32 positions differ from
                    // these by far less than one sample.
                    let reach = support
                        .map_or(0.0, |support| f64::from(support) * footprint(m, axis) + 1.0);
                    low[axis] = low[axis].min(at - reach);
                    high[axis] = high[axis].max(at + reach);
                }
            }
        }
        let axis = |a: usize| {
            let last = f64::from(from.extent[a]) - 1.0;
            let start = low[a].floor().clamp(0.0, last);
            let end = high[a].ceil().clamp(start, last);
            (start as u32, end as u32 - start as u32 + 1)
        };
        let ((x, width), (y, height)) = (axis(0), axis(1));
        Ok(Rect { origin: [x, y], extent: [width, height] })
    }

    /// How the kernel reads the input from lattice `from` onto `to`, and the
    /// per-channel index maps it reads them through. Reindexes are snapped to
    /// exact integers, so a filter sees taps exactly on samples.
    fn stage(&self, from: Lattice, to: Lattice) -> Result<(Stage, [Projective; 3])> {
        if let Some((body, _)) = self.stencil {
            return Ok((Stage::Stencil(body), self.maps));
        }
        let index = self.maps.each_ref().map(|m| index(m, from, to));
        let snapped = index.each_ref().map(|m| reindex(m, from, to));
        let maps = std::array::from_fn(|c| snapped[c].unwrap_or(index[c]));
        let identity = from.extent == to.extent && snapped.iter().all(|m| *m == Some(IDENTITY));
        // An interpolating filter reproduces samples at their own positions.
        if identity && self.filter.is_none_or(|f| f.interpolating) {
            return Ok((Stage::Load, maps));
        }
        match self.filter {
            Some(filter) => Ok((Stage::Filter(*filter), maps)),
            None if snapped.iter().all(Option::is_some) => Ok((Stage::Gather, maps)),
            None => Err(Error::Runtime("composed reindexes move samples off the lattice".into())),
        }
    }
}

/// A stencil's radius on the lattice it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Reach {
    /// Samples per axis, `ceil(radius · extent / full)` and at least one.
    samples: [u32; 2],
    /// Samples per full-detail pixel per axis, `extent / full`.
    rate: [f32; 2],
    /// On an axis where the lattice is coarser than full detail, the radius
    /// spans less than one sample, so the stencil's effect at full detail
    /// falls between the samples of this preview.
    approximates: bool,
}
impl Reach {
    fn of(radius: f32, lattice: Lattice) -> Self {
        let rate = |a: usize| f64::from(lattice.extent[a]) / f64::from(lattice.full[a]);
        let span = |a: usize| f64::from(radius) * rate(a);
        Self {
            samples: [0, 1].map(|a| (span(a).ceil() as u32).max(1)),
            rate: [0, 1].map(|a| rate(a) as f32),
            approximates: (0..2).any(|a| lattice.extent[a] < lattice.full[a] && span(a) < 1.0),
        }
    }

    /// `window` grown by the reach on each side, clamped to a lattice of
    /// `extent` samples.
    fn grow(&self, window: Rect, extent: [u32; 2]) -> Rect {
        let end = window.end();
        let start = [0, 1].map(|a| window.origin[a].saturating_sub(self.samples[a]));
        let end = [0, 1].map(|a| end[a].saturating_add(self.samples[a]).min(extent[a]));
        Rect { origin: start, extent: [0, 1].map(|a| end[a] - start[a]) }
    }
}

/// Whether `warp` takes every sample of lattice `to` onto a sample of lattice
/// `from` without leaving it. Such a warp commutes with per-pixel maps. Fails
/// when a warp declared as a reindex does not.
pub(crate) fn reindexes(warp: &Warp, from: Lattice, to: Lattice) -> Result<bool> {
    let reindex = warp.maps.iter().all(|m| reindex(&index(m, from, to), from, to).is_some());
    if warp.filter.is_none() && !reindex {
        return Err(Error::Contract("a reindex warp moves samples off the lattice".into()));
    }
    Ok(reindex)
}

/// `plane`, a map from the plane of `to` into the plane of `from`, as a map
/// from sample indices of `to` to sample indices of `from`. Folding both
/// conversions into the map keeps the kernel to one projective map per
/// channel and keeps exact maps exact in f64.
fn index(plane: &Projective, from: Lattice, to: Lattice) -> Projective {
    let step = |axis: usize| f64::from(to.full[axis]) / f64::from(to.extent[axis]);
    let (sx, sy) = (step(0), step(1));
    let to_plane = [[sx, 0.0, 0.5 * sx], [0.0, sy, 0.5 * sy], [0.0, 0.0, 1.0]];
    let rate = |axis: usize| f64::from(from.extent[axis]) / f64::from(from.full[axis]);
    let to_index = [[rate(0), 0.0, -0.5], [0.0, rate(1), -0.5], [0.0, 0.0, 1.0]];
    mul(&to_index, &mul(plane, &to_plane))
}

/// The index map `m` snapped to a reindex: a signed permutation of the axes
/// with integer offsets that takes every sample of `to` onto a sample of
/// `from`. Composition in f64 leaves exact maps about 1e-16 relative off,
/// which the snap absorbs.
fn reindex(m: &Projective, from: Lattice, to: Lattice) -> Option<Projective> {
    let snap = |v: f64| {
        let r = v.round();
        ((v - r).abs() <= 1e-9 * r.abs().max(1.0)).then_some(r)
    };
    let mut s = IDENTITY;
    for (row, snapped) in m.iter().zip(&mut s) {
        for (v, snapped) in row.iter().zip(snapped) {
            *snapped = snap(*v)?;
        }
    }
    let unit = |a: f64, b: f64| a.abs() + b.abs() == 1.0 && a * b == 0.0;
    let permutation = s[2] == [0.0, 0.0, 1.0]
        && unit(s[0][0], s[0][1])
        && unit(s[1][0], s[1][1])
        && unit(s[0][0], s[1][0])
        && unit(s[0][1], s[1][1]);
    let last = |l: Lattice, axis: usize| f64::from(l.extent[axis]) - 1.0;
    let inside = [[0.0, 0.0], [last(to, 0), 0.0], [0.0, last(to, 1)], [last(to, 0), last(to, 1)]]
        .iter()
        .all(|&[x, y]| {
            let (u, v) = (s[0][0] * x + s[0][1] * y + s[0][2], s[1][0] * x + s[1][1] * y + s[1][2]);
            (0.0..=last(from, 0)).contains(&u) && (0.0..=last(from, 1)).contains(&v)
        });
    (permutation && inside).then_some(s)
}

/// Input samples per output sample along `axis` of the index map `m`, as
/// the kernel clamps it. An affine map's is constant, a projective map's
/// varies, so its bound is the clamp.
fn footprint(m: &Projective, axis: usize) -> f64 {
    let max = f64::from(MAX_FOOTPRINT);
    if m[2][0] != 0.0 || m[2][1] != 0.0 {
        return max;
    }
    m[axis][0].hypot(m[axis][1]).clamp(1.0, max)
}

/// The input's stored samples as the kernels address them.
fn stored(from: Lattice) -> kernel::StoredLaunch {
    let [x, y] = from.window.origin.map(|v| v as usize);
    let [width, height] = from.window.extent.map(|v| v as usize);
    let [lattice_width, lattice_height] = from.extent.map(|v| v as usize);
    kernel::StoredLaunch::new(x, y, width, height, lattice_width, lattice_height)
}

/// The output samples a kernel computes: `to`'s window.
fn domain(to: Lattice) -> kernel::DomainLaunch {
    let [x, y] = to.window.origin.map(|v| v as usize);
    let [width, height] = to.window.extent.map(|v| v as usize);
    kernel::DomainLaunch::new(x, y, width, height)
}

/// Lanes per workgroup of the reduction kernels. The CPU runtime runs a
/// host worker per lane for kernels with barriers, so the runtime's limit
/// caps the 256 the GPUs take.
fn lanes(client: &Client) -> CubeDim {
    CubeDim::new_1d(client.properties().hardware.max_units_per_cube.min(256))
}

/// Workgroups of `dim` that cover `len` lanes.
fn grid(len: usize, dim: CubeDim) -> CubeCount {
    let groups = len.div_ceil(dim.num_elems() as usize);
    let x = groups.min(65535);
    CubeCount::Static(x as u32, groups.div_ceil(x) as u32, 1)
}

fn compose(a: &[Projective; 3], b: &[Projective; 3]) -> [Projective; 3] {
    std::array::from_fn(|c| mul(&a[c], &b[c]))
}

fn mul(a: &Projective, b: &Projective) -> Projective {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// How the fused kernel reads its input, part of its CubeCL kernel identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Stage {
    /// The output lattice is the input's: output sample `i` loads input sample `i`.
    Load,
    /// Each output sample copies the input sample a reindex takes it to.
    Gather,
    /// Each output sample filters the input around its mapped position.
    Filter(FilterBody),
    /// Each output sample is the stencil body over the input's window
    /// around it, on the same lattice.
    Stencil(StencilBody),
}
impl Stage {
    /// Constants the stage reads ahead of the map bodies' constants: a 3×3
    /// row-major index map per channel when it gathers or filters, a
    /// stencil's reach and rate per axis and then its body's constants.
    fn constants(self) -> usize {
        match self {
            Self::Load => 0,
            Self::Gather | Self::Filter(_) => 27,
            Self::Stencil(body) => 4 + body.constants,
        }
    }
}

/// The stage and body sequence of a fused kernel, its CubeCL kernel
/// identity. Constant windows follow from them, so equal sequences compile to
/// the same kernel whatever the constant values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Kernel {
    stage: Stage,
    bodies: Vec<MapBody>,
}
impl Kernel {
    /// Constants the stage and the map bodies read, ahead of a reduction's.
    fn constants(&self) -> usize {
        self.stage.constants() + self.bodies.iter().map(|body| body.constants).sum::<usize>()
    }
}

mod kernel {
    use super::*;

    /// One output pixel per invocation: resample, then apply the bodies.
    /// Positions are whole-lattice indices, so a window computes the same
    /// values as the whole image.
    #[cube(launch)]
    pub fn fused(
        input: &[f32],
        k: &[f32],
        output: &mut [f32],
        stored: Stored,
        domain: Domain,
        #[comptime] kernel: Kernel,
    ) {
        let p = ABSOLUTE_POS;
        if p * 3 < output.len() {
            let px = evaluate(input, k, p, &stored, &domain, &kernel);
            output[p * 3] = px.r;
            output[p * 3 + 1] = px.g;
            output[p * 3 + 2] = px.b;
        }
    }

    /// Lane `i` folds pixels `i`, `i + lanes`, … of the domain into an
    /// accumulator that starts at `body.identity`, read from `k` after the
    /// plan's constants, and writes it to `partial[i]`.
    #[cube(launch)]
    pub fn fold(
        input: &[f32],
        k: &[f32],
        partial: &mut [f32],
        stored: Stored,
        domain: Domain,
        #[comptime] kernel: Kernel,
        #[comptime] body: ReduceBody,
    ) {
        let width = comptime!(body.identity.len());
        let start = comptime!(kernel.constants());
        let lanes = partial.len() / width;
        let i = ABSOLUTE_POS;
        if i < lanes {
            let mut acc = Array::<f32>::new(width);
            for j in 0..width {
                acc[j] = k[start + j];
            }
            let constants = k.slice(start + width, k.len());
            let cells = domain.width * domain.height;
            let mut p = i;
            while p < cells {
                let px = evaluate(input, k, p, &stored, &domain, &kernel);
                accumulate(px, constants, acc.as_mut_slice(), body);
                p += lanes;
            }
            for j in 0..width {
                partial[i * width + j] = acc[j];
            }
        }
    }

    /// Lane `i` combines accumulators `i`, `i + lanes`, … of `partial` in
    /// that order into `output[i]`.
    #[cube(launch)]
    pub fn combine(partial: &[f32], output: &mut [f32], #[comptime] body: ReduceBody) {
        let width = comptime!(body.identity.len());
        let lanes = output.len() / width;
        let count = partial.len() / width;
        let i = ABSOLUTE_POS;
        if i < lanes {
            let mut acc = Array::<f32>::new(width);
            for j in 0..width {
                acc[j] = partial[i * width + j];
            }
            let mut p = i + lanes;
            while p < count {
                merge_body(acc.as_mut_slice(), partial.slice(p * width, p * width + width), body);
                p += lanes;
            }
            for j in 0..width {
                output[i * width + j] = acc[j];
            }
        }
    }

    /// Workgroup `g` counts every pixel of the domain it strides over into
    /// shared counts, then writes them to `partial[g]`.
    #[cube(launch)]
    pub fn count_groups(
        input: &[f32],
        k: &[f32],
        partial: &mut [u32],
        stored: Stored,
        domain: Domain,
        #[comptime] kernel: Kernel,
        #[comptime] body: BinsBody,
    ) {
        let len = comptime!(body.bins * body.channels);
        let mut counts = Shared::<[Atomic<u32>]>::new_slice(len);
        let mut i = UNIT_POS as usize;
        while i < len {
            counts[i].store(0u32);
            i += CUBE_DIM as usize;
        }
        sync_cube();
        let cells = domain.width * domain.height;
        let mut p = ABSOLUTE_POS;
        while p < cells {
            let bin = bin_of(input, k, p, &stored, &domain, &kernel, body);
            tally(&mut counts, bin.c0, 0usize, body);
            if comptime!(body.channels > 1) {
                tally(&mut counts, bin.c1, 1usize, body);
            }
            if comptime!(body.channels > 2) {
                tally(&mut counts, bin.c2, 2usize, body);
            }
            p += CUBE_COUNT * (CUBE_DIM as usize);
        }
        sync_cube();
        let mut i = UNIT_POS as usize;
        while i < len {
            partial[CUBE_POS * len + i] = counts[i].load();
            i += CUBE_DIM as usize;
        }
    }

    #[cube]
    fn tally(
        counts: &mut [Atomic<u32>],
        bin: u32,
        #[comptime] c: usize,
        #[comptime] body: BinsBody,
    ) {
        let bin = usize::cast_from(bin);
        if bin < comptime!(body.bins) {
            counts[bin * comptime!(body.channels) + c].fetch_add(1u32);
        }
    }

    /// Lane `l` counts the pixels of the domain it strides over into shard
    /// `l % shards` of `counts`, which starts at zero. Consecutive pixels of
    /// a lane in one bin add once, as a run.
    #[cube(launch)]
    pub fn count_shards(
        input: &[f32],
        k: &[f32],
        counts: &mut [Atomic<u32>],
        stored: Stored,
        domain: Domain,
        shards: usize,
        #[comptime] kernel: Kernel,
        #[comptime] body: BinsBody,
    ) {
        let shard = (UNIT_POS as usize % shards) * comptime!(body.bins * body.channels);
        // Per channel, the bin of the current run and its length.
        let mut previous = Array::<u32>::new(3usize);
        let mut run = Array::<u32>::new(3usize);
        for c in 0..3usize {
            previous[c] = u32::MAX;
            run[c] = 0u32;
        }
        let cells = domain.width * domain.height;
        let mut p = ABSOLUTE_POS;
        while p < cells {
            let bin = bin_of(input, k, p, &stored, &domain, &kernel, body);
            extend(
                counts,
                shard,
                previous.as_mut_slice(),
                run.as_mut_slice(),
                bin.c0,
                0usize,
                body,
            );
            if comptime!(body.channels > 1) {
                extend(
                    counts,
                    shard,
                    previous.as_mut_slice(),
                    run.as_mut_slice(),
                    bin.c1,
                    1usize,
                    body,
                );
            }
            if comptime!(body.channels > 2) {
                extend(
                    counts,
                    shard,
                    previous.as_mut_slice(),
                    run.as_mut_slice(),
                    bin.c2,
                    2usize,
                    body,
                );
            }
            p += CUBE_COUNT * (CUBE_DIM as usize);
        }
        flush(counts, shard, previous.as_mut_slice(), run.as_mut_slice(), 0usize, body);
        if comptime!(body.channels > 1) {
            flush(counts, shard, previous.as_mut_slice(), run.as_mut_slice(), 1usize, body);
        }
        if comptime!(body.channels > 2) {
            flush(counts, shard, previous.as_mut_slice(), run.as_mut_slice(), 2usize, body);
        }
    }

    /// Count channel `c` of a pixel in `bin`, ending the channel's run when
    /// the bin changes.
    #[cube]
    fn extend(
        counts: &mut [Atomic<u32>],
        shard: usize,
        previous: &mut [u32],
        run: &mut [u32],
        bin: u32,
        #[comptime] c: usize,
        #[comptime] body: BinsBody,
    ) {
        if bin != previous[c] {
            flush(counts, shard, previous, run, c, body);
            previous[c] = bin;
            run[c] = 0u32;
        }
        run[c] += 1u32;
    }

    /// Add channel `c`'s run to its bin, unless the bin counts nothing.
    #[cube]
    fn flush(
        counts: &mut [Atomic<u32>],
        shard: usize,
        previous: &mut [u32],
        run: &mut [u32],
        #[comptime] c: usize,
        #[comptime] body: BinsBody,
    ) {
        let bin = usize::cast_from(previous[c]);
        if run[c] > 0u32 && bin < comptime!(body.bins) {
            counts[shard + bin * comptime!(body.channels) + c].fetch_add(run[c]);
        }
    }

    #[cube(launch)]
    pub fn clear(output: &mut [u32]) {
        if ABSOLUTE_POS < output.len() {
            output[ABSOLUTE_POS] = 0u32;
        }
    }

    /// `output[i]` is the sum of element `i` of each of the `parts` arrays
    /// of `output.len()` counts in `partial`.
    #[cube(launch)]
    pub fn merge(partial: &[u32], output: &mut [u32], parts: usize) {
        let i = ABSOLUTE_POS;
        if i < output.len() {
            let mut sum = 0u32;
            for part in 0..parts {
                sum += partial[part * output.len() + i];
            }
            output[i] = sum;
        }
    }

    /// Pixel `p` of the domain, row-major, as the plan computes it: resample
    /// at its whole-lattice index, then apply the bodies. Every kernel of a
    /// plan evaluates pixels through this.
    #[cube]
    fn evaluate(
        input: &[f32],
        k: &[f32],
        p: usize,
        stored: &Stored,
        domain: &Domain,
        #[comptime] kernel: &Kernel,
    ) -> Pixel {
        let x = p % domain.width + domain.x;
        let y = p / domain.width + domain.y;
        let px = sample(input, k, x, y, stored, comptime!(kernel.stage));
        apply(px, k, kernel)
    }

    /// The bins of pixel `p` of the domain under `body`, whose constants
    /// follow the plan's in `k`.
    #[cube]
    fn bin_of(
        input: &[f32],
        k: &[f32],
        p: usize,
        stored: &Stored,
        domain: &Domain,
        #[comptime] kernel: &Kernel,
        #[comptime] body: BinsBody,
    ) -> Bin {
        let px = evaluate(input, k, p, stored, domain, kernel);
        let at = Position {
            x: u32::cast_from(p % domain.width),
            y: u32::cast_from(p / domain.width),
            width: u32::cast_from(domain.width),
            height: u32::cast_from(domain.height),
        };
        let constants = k.slice(comptime!(kernel.constants()), k.len());
        bin(px, at, constants, body)
    }

    #[cube]
    fn accumulate(px: Pixel, k: &[f32], acc: &mut [f32], #[comptime] body: ReduceBody) {
        intrinsic!(|scope| (body.accumulate)(scope, px, k, acc))
    }

    #[cube]
    fn merge_body(acc: &mut [f32], other: &[f32], #[comptime] body: ReduceBody) {
        intrinsic!(|scope| (body.combine)(scope, acc, other))
    }

    #[cube]
    fn bin(px: Pixel, at: Position, k: &[f32], #[comptime] body: BinsBody) -> Bin {
        intrinsic!(|scope| (body.bin)(scope, px, at, k))
    }

    /// Where the input's samples sit: `width × height` stored from `(x, y)`
    /// of a lattice of `lattice_width × lattice_height` samples.
    #[derive(CubeType, CubeLaunch)]
    pub struct Stored {
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        lattice_width: usize,
        lattice_height: usize,
    }

    /// The samples a kernel computes: `width × height` from `(x, y)` of the
    /// output lattice.
    #[derive(CubeType, CubeLaunch)]
    pub struct Domain {
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    }

    #[cube]
    fn sample<'a>(
        input: &'a [f32],
        k: &[f32],
        x: usize,
        y: usize,
        stored: &Stored,
        #[comptime] stage: Stage,
    ) -> Pixel {
        let (fx, fy) = (f32::cast_from(x), f32::cast_from(y));
        #[comptime]
        match stage {
            Stage::Load => {
                let i = ((y - stored.y) * stored.width + x - stored.x) * 3;
                Pixel { r: input[i], g: input[i + 1], b: input[i + 2] }
            }
            Stage::Gather => Pixel {
                r: gather(input, k, fx, fy, stored, 0usize),
                g: gather(input, k, fx, fy, stored, 1usize),
                b: gather(input, k, fx, fy, stored, 2usize),
            },
            Stage::Filter(filter) => Pixel {
                r: filtered(input, k, fx, fy, stored, 0usize, filter),
                g: filtered(input, k, fx, fy, stored, 1usize, filter),
                b: filtered(input, k, fx, fy, stored, 2usize, filter),
            },
            Stage::Stencil(body) => {
                let w = Window::<'a> {
                    input,
                    x: i32::cast_from(x),
                    y: i32::cast_from(y),
                    left: i32::cast_from(stored.x),
                    top: i32::cast_from(stored.y),
                    stride: i32::cast_from(stored.width),
                    width: i32::cast_from(stored.lattice_width),
                    height: i32::cast_from(stored.lattice_height),
                    radius_x: i32::cast_from(k[0]),
                    radius_y: i32::cast_from(k[1]),
                    rate_x: k[2],
                    rate_y: k[3],
                    border: comptime!(body.border),
                };
                stencil(&w, k.slice(4, comptime!(4 + body.constants)), body)
            }
        }
    }

    #[cube]
    fn stencil(w: &Window, k: &[f32], #[comptime] body: StencilBody) -> Pixel {
        intrinsic!(|scope| (body.expand)(scope, w, k))
    }

    /// Channel `c` of the input sample that the reindex in `k` takes output
    /// sample `(x, y)` to. Reindexes are affine with integer entries, so the
    /// position is an exact integer in f32.
    #[cube]
    fn gather(
        input: &[f32],
        k: &[f32],
        x: f32,
        y: f32,
        stored: &Stored,
        #[comptime] c: usize,
    ) -> f32 {
        let m = c * 9;
        let u = usize::cast_from(k[m] * x + k[m + 1] * y + k[m + 2]) - stored.x;
        let v = usize::cast_from(k[m + 3] * x + k[m + 4] * y + k[m + 5]) - stored.y;
        input[(v * stored.width + u) * 3 + c]
    }

    /// Channel `c` at output sample `(x, y)` resampled after Heckbert: the
    /// projective map in `k` takes it to the input, where `filter` is centred
    /// and widened by the map's local derivative, so magnifying interpolates
    /// and minifying antialiases with one filter. Weights are normalized.
    /// Positions outside the input lattice or beyond the map's horizon
    /// (`w ≤ 0`) are black, and taps beyond its edge repeat the edge sample.
    /// Taps clamp to the stored window, which holds every tap inside the
    /// lattice ([`Plan::input_window`]).
    #[cube]
    fn filtered(
        input: &[f32],
        k: &[f32],
        x: f32,
        y: f32,
        stored: &Stored,
        #[comptime] c: usize,
        #[comptime] filter: FilterBody,
    ) -> f32 {
        let m = c * 9;
        let support = comptime!(filter.support);
        let hw = k[m + 6] * x + k[m + 7] * y + k[m + 8];
        let mut value = 0.0f32;
        if hw > 0.0f32 {
            let u = (k[m] * x + k[m + 1] * y + k[m + 2]) / hw;
            let v = (k[m + 3] * x + k[m + 4] * y + k[m + 5]) / hw;
            let last_u = f32::cast_from(stored.lattice_width) - 0.5f32;
            let last_v = f32::cast_from(stored.lattice_height) - 0.5f32;
            if u >= -0.5f32 && u <= last_u && v >= -0.5f32 && v <= last_v {
                // Derivatives of the projective map: the source extent of
                // one output step along each source axis sets the footprint.
                // Heckbert fits an ellipse (EWA). This scales the filter per
                // source axis by the length of each Jacobian row.
                let ux = (k[m] - u * k[m + 6]) / hw;
                let uy = (k[m + 1] - u * k[m + 7]) / hw;
                let vx = (k[m + 3] - v * k[m + 6]) / hw;
                let vy = (k[m + 4] - v * k[m + 7]) / hw;
                let su = f32::clamp((ux * ux + uy * uy).sqrt(), 1.0f32, MAX_FOOTPRINT);
                let sv = f32::clamp((vx * vx + vy * vy).sqrt(), 1.0f32, MAX_FOOTPRINT);
                let u0 = i32::cast_from((u - support * su).ceil());
                let u1 = i32::cast_from((u + support * su).floor());
                let v0 = i32::cast_from((v - support * sv).ceil());
                let v1 = i32::cast_from((v + support * sv).floor());
                let min_col = i32::cast_from(stored.x);
                let min_row = i32::cast_from(stored.y);
                let max_col = min_col + i32::cast_from(stored.width) - 1i32;
                let max_row = min_row + i32::cast_from(stored.height) - 1i32;
                // −0 is the additive identity, so an exact hit keeps −0.
                let mut sum = f32::reinterpret(0x8000_0000u32);
                let mut total = 0.0f32;
                for b in v0..v1 + 1 {
                    let wy = weight((f32::cast_from(b) - v) / sv, filter);
                    let row =
                        usize::cast_from(i32::clamp(b, min_row, max_row) - min_row) * stored.width;
                    for a in u0..u1 + 1 {
                        let w = weight((f32::cast_from(a) - u) / su, filter) * wy;
                        // Zero weights skip their tap, so non-finite
                        // neighbours of an exact hit cannot leak in.
                        if w != 0.0f32 {
                            let col = usize::cast_from(i32::clamp(a, min_col, max_col) - min_col);
                            sum += w * input[(row + col) * 3 + c];
                            total += w;
                        }
                    }
                }
                if total != 0.0f32 {
                    value = sum / total;
                }
            }
        }
        value
    }

    #[cube]
    fn weight(x: f32, #[comptime] filter: FilterBody) -> f32 {
        intrinsic!(|scope| (filter.expand)(scope, x))
    }

    /// Apply each body to `px` with its window of `k`, after the stage's
    /// maps. The loop runs while the kernel is expanded, so the compiled
    /// kernel holds the bodies inline.
    #[cube]
    fn apply(px: Pixel, k: &[f32], #[comptime] kernel: &Kernel) -> Pixel {
        intrinsic!(|scope| {
            let mut px = px;
            let mut offset = kernel.stage.constants();
            for body in &kernel.bodies {
                let end = offset + body.constants;
                let window = k.__expand_slice_method(scope, offset.into(), end.into());
                px = (body.expand)(scope, px, window);
                offset = end;
            }
            px
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lattice of `extent` samples over a plane of `full` pixels.
    fn lattice(extent: [u32; 2], full: [u32; 2]) -> Lattice {
        Lattice::whole(extent, full)
    }

    #[test]
    fn a_reach_rounds_up_to_samples_and_reports_sub_sample_previews() {
        let full = Reach::of(4.0, lattice([64, 48], [64, 48]));
        assert_eq!((full.samples, full.approximates), ([4, 4], false));
        let half = Reach::of(4.0, lattice([32, 24], [64, 48]));
        assert_eq!((half.samples, half.rate, half.approximates), ([2, 2], [0.5, 0.5], false));
        // `div_ceil` leaves 51 samples over 101 pixels: 2.02 samples.
        assert_eq!(Reach::of(4.0, lattice([51, 24], [101, 48])).samples, [3, 2]);
        // Below one sample only at a preview, and at least one sample.
        assert_eq!(Reach::of(0.5, lattice([64, 48], [64, 48])).samples, [1, 1]);
        assert!(!Reach::of(0.5, lattice([64, 48], [64, 48])).approximates);
        assert!(Reach::of(1.5, lattice([16, 12], [64, 48])).approximates);
    }

    #[test]
    fn a_stencil_reads_its_window_grown_by_the_reach_within_the_lattice() {
        let plan = Plan::default().stencil(&BODY, 3.0, &[]).unwrap();
        let whole = lattice([40, 30], [40, 30]);
        let window = |origin, extent| whole.with(Rect { origin, extent });
        let read = plan.input_window(whole, window([10, 10], [5, 5])).unwrap();
        assert_eq!(read, Rect { origin: [7, 7], extent: [11, 11] });
        let read = plan.input_window(whole, window([0, 28], [4, 2])).unwrap();
        assert_eq!(read, Rect { origin: [0, 25], extent: [7, 5] });
    }

    #[test]
    fn a_stencil_starts_a_plan_and_only_maps_join_it() {
        let filtered = Plan::default().resample(&crate::body::BOX, LATTICE, LATTICE).unwrap();
        assert!(filtered.stencil(&BODY, 1.0, &[]).is_none());
        let plan = Plan::default().stencil(&BODY, 1.0, &[]).unwrap();
        assert!(plan.resample(&crate::body::BOX, LATTICE, LATTICE).is_none());
        let reindex = Warp::reindex(IDENTITY);
        assert!(plan.warp(&reindex, true).is_none());
        assert_eq!(plan.stage(LATTICE, LATTICE).unwrap().0, Stage::Stencil(BODY));
    }

    const LATTICE: Lattice =
        Lattice { extent: [8, 8], full: [8, 8], window: Rect { origin: [0; 2], extent: [8, 8] } };

    static BODY: StencilBody = StencilBody {
        name: "plan::tests::centre",
        constants: 0,
        border: crate::body::Border::Zero,
        expand: centre::expand,
    };

    #[cube]
    fn centre(w: &Window, _k: &[f32]) -> Pixel {
        w.at(0i32, 0i32)
    }
}
