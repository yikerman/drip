//! The segment compiler: warps, a resample and the maps after them run as
//! one kernel.
//!
//! Map, warp and resample nodes publish deferred images. A [`Plan`] holds one
//! resampling stage, per-channel projective maps from the image's plane to
//! its input raster's plane with their filter, then the map bodies. The fused
//! kernel resamples each output pixel, applies the bodies in order and stores
//! the result, so intermediate images never reach memory. A step composes
//! into a plan only where the result is exact:
//!
//! - A reindex (Orientation) commutes with per-pixel maps and joins the stage
//!   wherever it sits.
//! - A filtering warp or a Resample after a map cuts the segment: the maps run
//!   on their own lattice, then the resampling runs as its own launch. A
//!   weighted average of sigmoid values is not the sigmoid of the average.
//!   Before any map they fuse.
//! - A Resample fixes the stage's lattice, so a filtering step after it
//!   resamples again.
//!
//! Preview scale reduces samples early, in demosaic. That approximation is
//! preview-only: at full detail, as export runs, every resampling follows
//! these rules.
use crate::{
    Error, Result,
    image::Raster,
    runtime::{KernelContext, dispatch_dims},
    shape::{FilterBody, Lattice, MapBody, MapPayload, Pixel, Projective, Warp},
};
use cubecl::prelude::*;
use std::sync::Arc;

/// Footprints wider than this many source samples per output sample are
/// clamped, bounding taps per output sample. Stronger reductions alias. A
/// prefiltered pyramid or EWA would lift the bound.
pub const MAX_FOOTPRINT: f32 = 16.0;

const IDENTITY: Projective = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A resampling stage followed by map bodies, with the bodies' constants
/// concatenated.
#[derive(Clone)]
pub(crate) struct Plan {
    /// Per channel, the image's plane to the input raster's plane.
    maps: [Projective; 3],
    /// The filter where `maps` move samples off the input lattice. `None`
    /// while every warp so far was a reindex.
    filter: Option<&'static FilterBody>,
    /// A Resample fixed the stage's filter and lattice.
    resampled: bool,
    bodies: Vec<MapBody>,
    constants: Vec<f32>,
}
impl Default for Plan {
    fn default() -> Self {
        Self {
            maps: [IDENTITY; 3],
            filter: None,
            resampled: false,
            bodies: Vec::new(),
            constants: Vec::new(),
        }
    }
}
impl Plan {
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
        (reindex || self.open()).then(|| Self {
            maps: compose(&self.maps, &warp.maps),
            filter,
            ..self.clone()
        })
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

    /// No body has run and no Resample fixed the stage, so a filtering step
    /// still composes into it.
    fn open(&self) -> bool {
        !self.resampled && self.bodies.is_empty()
    }

    /// Run the plan on `input` into a new raster described by `desc`. A plan
    /// that neither moves nor maps samples shares the input's samples instead.
    pub fn run<P: MapPayload>(
        &self,
        input: &Raster<P>,
        desc: Arc<P::Desc>,
        ctx: &KernelContext<'_>,
    ) -> Result<Raster<P>> {
        let (from, to) = (P::lattice(input.desc()), P::lattice(&desc));
        let (stage, maps) = self.stage(from, to)?;
        if stage == Stage::Load && self.bodies.is_empty() {
            return input.share_as(desc);
        }
        let mut constants: Vec<f32> = match stage {
            Stage::Load => Vec::new(),
            Stage::Gather | Stage::Filter(_) => {
                maps.iter().flat_map(|m| m.as_flattened()).map(|&v| v as f32).collect()
            }
        };
        if constants.iter().any(|v| !v.is_finite()) {
            return Err(Error::Contract("warp maps do not fit f32".into()));
        }
        constants.extend_from_slice(&self.constants);
        let constants = ctx.constants(&constants)?;
        let output = Raster::from_device(desc.clone(), ctx.device_buffer::<P>(&desc)?, ctx)?;
        let [width, height] = from.extent.map(|v| v as usize);
        let out_width = to.extent[0] as usize;
        let (count, dim) = dispatch_dims(out_width * to.extent[1] as usize);
        kernel::fused::launch(
            ctx.client()?,
            count,
            dim,
            input.device(ctx)?.argument(),
            constants.argument(),
            output.device(ctx)?.argument(),
            width,
            height,
            out_width,
            Kernel { stage, bodies: self.bodies.clone() },
        );
        ctx.runtime.count_fused();
        Ok(output)
    }

    /// How the kernel reads the input from lattice `from` onto `to`, and the
    /// per-channel index maps it reads them through. Reindexes are snapped to
    /// exact integers, so a filter sees taps exactly on samples.
    fn stage(&self, from: Lattice, to: Lattice) -> Result<(Stage, [Projective; 3])> {
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
}
impl Stage {
    /// Constants the stage reads ahead of the bodies' constants: a 3×3
    /// row-major index map per channel, unless it loads.
    fn constants(self) -> usize {
        if self == Self::Load { 0 } else { 27 }
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

mod kernel {
    use super::*;

    /// One output pixel per invocation: resample, then apply the bodies.
    #[cube(launch)]
    pub fn fused(
        input: &[f32],
        k: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
        out_width: usize,
        #[comptime] kernel: Kernel,
    ) {
        let p = ABSOLUTE_POS;
        if p * 3 < output.len() {
            let px = sample(input, k, p, width, height, out_width, comptime!(kernel.stage));
            let px = apply(px, k, &kernel);
            output[p * 3] = px.r;
            output[p * 3 + 1] = px.g;
            output[p * 3 + 2] = px.b;
        }
    }

    #[cube]
    fn sample(
        input: &[f32],
        k: &[f32],
        p: usize,
        width: usize,
        height: usize,
        out_width: usize,
        #[comptime] stage: Stage,
    ) -> Pixel {
        let x = f32::cast_from(p % out_width);
        let y = f32::cast_from(p / out_width);
        #[comptime]
        match stage {
            Stage::Load => Pixel { r: input[p * 3], g: input[p * 3 + 1], b: input[p * 3 + 2] },
            Stage::Gather => Pixel {
                r: gather(input, k, x, y, width, 0usize),
                g: gather(input, k, x, y, width, 1usize),
                b: gather(input, k, x, y, width, 2usize),
            },
            Stage::Filter(filter) => Pixel {
                r: filtered(input, k, x, y, width, height, 0usize, filter),
                g: filtered(input, k, x, y, width, height, 1usize, filter),
                b: filtered(input, k, x, y, width, height, 2usize, filter),
            },
        }
    }

    /// Channel `c` of the input sample that the reindex in `k` takes output
    /// sample `(x, y)` to. Reindexes are affine with integer entries, so the
    /// position is an exact integer in f32.
    #[cube]
    fn gather(input: &[f32], k: &[f32], x: f32, y: f32, width: usize, #[comptime] c: usize) -> f32 {
        let m = c * 9;
        let u = k[m] * x + k[m + 1] * y + k[m + 2];
        let v = k[m + 3] * x + k[m + 4] * y + k[m + 5];
        input[(usize::cast_from(v) * width + usize::cast_from(u)) * 3 + c]
    }

    /// Channel `c` at output sample `(x, y)` resampled after Heckbert: the
    /// projective map in `k` takes it to the input, where `filter` is centred
    /// and widened by the map's local derivative, so magnifying interpolates
    /// and minifying antialiases with one filter. Weights are normalized.
    /// Positions outside the input or beyond the map's horizon (`w ≤ 0`) are
    /// black, and taps beyond the edge repeat the edge sample.
    #[cube]
    #[allow(clippy::too_many_arguments)]
    fn filtered(
        input: &[f32],
        k: &[f32],
        x: f32,
        y: f32,
        width: usize,
        height: usize,
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
            let last_u = f32::cast_from(width) - 0.5f32;
            let last_v = f32::cast_from(height) - 0.5f32;
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
                let max_col = i32::cast_from(width) - 1i32;
                let max_row = i32::cast_from(height) - 1i32;
                // −0 is the additive identity, so an exact hit keeps −0.
                let mut sum = f32::reinterpret(0x8000_0000u32);
                let mut total = 0.0f32;
                for b in v0..v1 + 1 {
                    let wy = weight((f32::cast_from(b) - v) / sv, filter);
                    let row = usize::cast_from(i32::clamp(b, 0i32, max_row)) * width;
                    for a in u0..u1 + 1 {
                        let w = weight((f32::cast_from(a) - u) / su, filter) * wy;
                        // Zero weights skip their tap, so non-finite
                        // neighbours of an exact hit cannot leak in.
                        if w != 0.0f32 {
                            let col = usize::cast_from(i32::clamp(a, 0i32, max_col));
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
