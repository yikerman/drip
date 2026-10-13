# Continuous fields: geometry and resampling

Design for resampling, perspective/rotation and later lens correction,
settled with the user on 2026-10-12. Nothing here is implemented; status
markers follow [DESIGN](DESIGN.md). Node, port and contract principles are in
[TYPE_SYSTEM](TYPE_SYSTEM.md).

## Summary

- **Decided:** An explicit node lifts discrete RGB samples into an `RgbField`,
  a continuous image. `RgbField -> RgbField` nodes only add declarations
  (coordinate maps, domain) to the description; they move no pixels. A
  `Sample` node does all filtering once and returns ordinary `Rgb`.
- **Decided:** Interpolation (magnification) and antialiasing (minification)
  are one resampling filter scaled by the local footprint; filter choice lives
  on `Sample` only.
- **Decided:** Maps may differ per channel (lateral chromatic aberration).
- **Decided:** v1 fields carry maps only. Per-pixel value operations (exposure,
  matrices, sigmoid) stay `Rgb -> Rgb`; see [Value operations](#value-operations).
- **Decided:** To view a field, wire it through a `Sample` into an ordinary
  view. Views do not read fields.
- **Decided:** Discrete operations (demosaic, denoise, sharpen, Orientation's
  exact permutations) stay as they are.
- **Tentative:** `MonoField` for single-channel data (masks, gain maps) when a
  consumer needs it; see [Applications](#applications).

Intended workflow:

```text
RGB -> Interpolate -> Lens correction -> Perspective Transform -> Sample -> RGB
       (Rgb→RgbField)  (RgbField→RgbField)  (RgbField→RgbField)  (RgbField→Rgb)
```

Delivery resizing ("Resample") is `Interpolate -> Sample` with a size.

## Mathematics

### Fields and maps

Discrete samples `s_k` at pixel centres `p_k` and a reconstruction kernel `r`
define a continuous image

```text
f_c(p) = Σ_k s_{k,c} r(p − p_k)          channel c
```

A geometric node with map `t` (its output plane → its input plane) produces
`F_c(y) = f_c(t(y))`. Successive nodes compose on the output side, so a chain
of nodes `t_1, …, t_n` (in graph order) gives

```text
F_c(y) = f_c(m_c(y)),   m_c = t_1 ∘ t_2 ∘ … ∘ t_n
```

Sample evaluates `t_n` first. Maps run from output to source, the direction
inverse mapping needs; lensfun's distortion and TCA models are stated in this
direction (undistorted → distorted) for the same reason [L1].

### Resampling

Sampling `F` on a new grid `y_j` with prefilter `h` is [1, ch. 4]

```text
out_{j,c} = ∫ h(y_j − y) F_c(y) dy = Σ_k s_{k,c} ρ_{j,c}(k)
```

The ideal kernel `ρ` is space-variant: it is `r` warped by `m_c` and
convolved with `h`. When the map magnifies, `r` dominates (interpolation);
when it minifies, `h` dominates (antialiasing). Both are one filter.

Approximation for v1, after OpenImageIO's `warp` [4]: one filter kernel `φ`
(box, triangle, Mitchell–Netravali [2], Lanczos-3), scaled per source axis by
the footprint from the map's Jacobian `J_c = ∂m_c/∂y` in source-pixel units,

```text
σ_x = max(1, |J_xx|, |J_xy|),   σ_y = max(1, |J_yx|, |J_yy|)
w_{j,c}(k) = φ((p_k − m_c(y_j)) ⊘ σ) / Σ_k φ(…)
```

Weights are normalized. This is isotropic per source axis and blurs strongly
anisotropic footprints (extreme keystone); elliptical weighted averaging [3]
is the measured upgrade path, not the starting point. Plain scaling is the
special case of a constant diagonal `J`, so Resample needs no second kernel;
a separable fast path is an optimization to measure, not a design element.

### Value operations

For a per-pixel operation `g`, applying it to samples before interpolating
equals applying it to the field when `g` commutes with the weighted sum:

- **Linear and channel-preserving** (exposure, per-channel gains): exact with
  any maps.
- **Channel mixing** (camera matrix, CAT16): exact only when all channels share
  one map. With per-channel maps, `Σ_d M_cd f_d(m_c(y)) ≠ Σ_d M_cd f_d(m_d(y))`:
  mixing before a TCA correction mixes misregistered channels. Document TCA
  correction as acting on sensor-registered channels; do not enforce an order.
- **Affine with offsets**: exact because weights sum to one.
- **Nonlinear** (sigmoid, curves, clipping): not exact. `g ∘ F` is not band
  limited, so prefiltering it needs `g` evaluated under the footprint
  (supersampling), the "filter after shading" problem [5], [6]. To first order,
  the field semantics amount to applying `g` at the higher of the two
  resolutions.

Linear operations therefore gain nothing from living inside a field: place them
before Interpolate or after Sample. Deferring nonlinear `g` needs runtime kernel
composition (an op interpreter over per-pixel `#[cube]` functions, or
Burn-style fusion [7]) and either duplicated node kinds or port types inferred
from connections. **Open**, revisit with a concrete quality case.

## Types

Sketch; names are tentative until implementation.

```rust
pub struct RgbDesc {
    pub extent: Extent,     // stored samples
    pub full: Extent,       // the same image at full detail (GlobalContext::scale = 1)
    pub coordinates: Coordinates,
}
pub struct Rect { pub origin: [f64; 2], pub size: [f64; 2] }
pub struct RgbFieldDesc {
    pub source: RgbDesc,        // the stored samples and their colour meaning
    pub domain: Rect,           // where the field is defined, output-plane units
    pub maps: [Chain; 3],       // output plane → source plane, per channel
    pub border: Border,         // f beyond the source rectangle
}
pub struct Chain(Vec<Stage>);   // newest stage first
pub enum Stage { Projective(Mat3), Radial(RadialModel) /* lens, later */ }
pub struct RgbField;            // Host = HostBuffer<[f32; 3]>, DeviceLayout as Rgb
```

- **Plane units:** full-detail pixels. Pixel `i` of a stored extent sits at
  plane coordinate `(i + 0.5) · full / extent`. `full` replaces
  reconstructing geometry as `extent · scale`, which is inexact after
  `div_ceil` reductions. Demosaic sets `full` from the sensor RGB extent;
  Orientation swaps it; reductions and Sample set it from their full-detail
  result.
- **Payload identity:** edges connect only on equal payload types
  (`graph.rs`), so `RgbField` is its own payload and `Rgb -> RgbField` needs an
  explicit node. The conversion is lossless (identity maps, same storage).
  `RgbField -> Rgb` filters and must stay an explicit node: by
  [TYPE_SYSTEM](TYPE_SYSTEM.md#representations-and-purity), a lossy
  computation is not transport.
- **Requirements:** geometry ports accept any coordinates. `Sample` preserves
  `source.coordinates`, so `Rec2020` requirements downstream still hold. Add
  `Requirement<RgbField>` impls only when a field port needs one.
- **Contracts:** everything deferred is in the description, so contracts check
  undefined transforms (singular homography, self-intersecting quad), compute
  domains and auto-crop rectangles on the host before allocation.
- **Pass-through outputs (needed):** today the `#[node]` adapter allocates
  every output (`OutputSlot::allocate`). A field node must publish its input's
  storage `Arc` with a new description. Add an output form, e.g.
  `Forward<'_, Device<RgbField>>`, set from an input of the same placement.
  Storage is already shared and immutable, so purity holds. Nodes that only
  reinterpret (assigning RGB meaning to camera numbers) can use it too.

## Nodes

| Node | Ports | Parameters | Description change |
|------|-------|-----------|---------------------|
| Interpolate | `Rgb -> RgbField` | border | identity maps; domain = `[0, full]` |
| Perspective Transform | `RgbField -> RgbField` | crop, angle, four-point quad, auto-crop | prepend `Projective`; new domain |
| Lens correction (later) | `RgbField -> RgbField` [+ metadata] | model, TCA | prepend per-channel `Radial` |
| Sample | `RgbField -> Rgb` | filter, size (factor or long edge) | realizes; output `full` from domain and size |

Sample sizing: full-detail output extent `N = round(domain.size · factor)` or
fit to a long edge; stored extent follows the RGB reduction rule
(`div_ceil(N, global.scale)`), so preview and export framing agree. Per output
pixel and channel the kernel evaluates the chain with forward-mode derivatives
(a point and its 2×2 Jacobian; the chain rule composes them), scales the
footprint, sums normalized taps and applies the border.

Always evaluate three chains rather than branching on whether maps are shared.
Taps load one channel each instead of three channels once: equal bytes, three
weight evaluations.

Borders: **Open.** Options are auto-crop to the largest axis-aligned rectangle
inside the mapped source (vkdt's crop offers this [V1]), a constant (vkdt and
OIIO default to black), or clamp/mirror. An alpha or coverage channel would be a
payload change. Points mapped through a projective pole (`w ≤ 0`) take the
border; only undefined transforms are rejected.

## Prior art

Read at pinned revisions on 2026-10-12.

- **darktable** `e43fbf3` [D1]: every distorting module (lens, rotate and
  perspective, liquify, clipping, rotate/scale pixels) inverse-maps and
  interpolates separately with the user's warp interpolator through
  `dt_interpolation_compute_pixel4c`, a fixed footprint. Only plain rescaling
  (`dt_interpolation_resample`, used by `finalscale`) widens taps when
  downscaling. `distort_transform`/`distort_backtransform` compose maps for
  points (masks, GUI), never for pixels. `dt_iop_roi_t {x, y, width, height,
  scale}` locates buffers in the full image.
- **vkdt** `f0be638` [V1]: `crop` combines crop, rotation and homography in one
  inverse-map pass (Catmull–Rom from bilinear taps, black outside, optional
  auto-crop). `lens` is a separate Catmull–Rom pass whose source notes
  "TODO: use textureGrad with analytic derivatives". `resize` uses fixed
  samplers. `dt_roi_t` carries `full_wd/full_ht` beside the scaled `wd/ht`,
  the precedent for `RgbDesc::full`. No cross-module fusion; `colour` merges
  matrix, white balance and RBF by design.
- **RawTherapee** `5f486d3` [R1]: `transformGeneral` composes lens-profile
  distortion and TCA, manual distortion, rotation, perspective and scale into
  one map, multiplies vignetting and gradient gains, and interpolates once
  (4×4 bicubic, optionally in log space). This is `sample(G · f ∘ m)` with
  linear `G`, in a fixed pipeline. Export resizes later in a separate Lanczos
  pass on L\*a\*b\* data (`simpleprocess.cc`), so transform and resize filter
  twice.
- **Nuke** [8]: consecutive transforms concatenate and filter once; colour
  nodes break concatenation.
- **Blender** realtime compositor [9]: transform nodes record the interpolation
  method; a later node realizes. A later pull request, "Immediately realize
  transformations" [10], changed this; its rationale was not read.
- **OpenImageIO** `ee1c641` [4]: `warp` propagates derivatives through the
  matrix and scales an isotropic filter by them.

Fixed pipelines get single filtering by construction. Drip gets it from the
graph when geometry nodes are adjacent; a discrete node between them forces a
Sample there, as a non-concatenating node does in Nuke. Help and templates
should say so; the graph does not forbid it.

## Applications

`RgbField -> RgbField` candidates beyond the workflow above:

- crop, straighten and keystone (Perspective Transform); anamorphic desqueeze
  (non-square pixels);
- lens distortion and lateral CA; projection changes (fisheye to rectilinear,
  cylindrical/equirectangular for panoramas);
- mesh warps (liquify), as a coarse-grid stage kind;
- registration for multi-frame work: map frame B onto frame A, then Sample on
  A's grid for HDR merge, focus stacking or burst denoise. Needs a Sample input
  that supplies the target grid.

`MonoField` (tentative): masks drawn in source coordinates follow the image's
geometry by passing through the same maps; coarse calibration grids (flat
field, vignetting gain) sample onto an image grid. Reusing one chain for an
image and its mask suggests promoting `Chain` to its own payload; **Open**.
Analytic fields (gradient masks with no samples) would make `source` an enum;
deferred until masks exist.

## Maintainability

- One source per map: a stage is one `#[cube]` function returning a point and
  Jacobian. `#[cube]` functions are callable from host Rust (`sigmoid.rs`
  samples its curve this way), so contracts, GUI handles and the kernel share
  it.
- Closed stage set: the kernel matches on a small enum with uniform values; no
  divergence and no general kernel fusion. Adding a map kind touches the enum,
  its function and host flattening into constants.
- Field nodes are contracts plus help; they allocate nothing.
- Sample is the single hotspot: filters, borders and footprints live there and
  are tested there.
- Risks: every stage needs the output→source map, and GUI handles need the
  forward map; lens models are closed-form in one direction, so the other
  needs Newton iteration with a stated tolerance. A crop costs three nodes;
  templates help. Runtime stage loops versus comptime-specialized kernels per
  chain shape (JIT on first use): start with the loop and measure. A composed
  identity lands about 1e-6 px off integer positions, so identity and
  integer-crop tests need tolerances.

## Implementation sequence

Each step is one self-contained commit series.

1. `RgbDesc::full` and the pixel-centre convention through demosaic, reduction,
   Orientation, camera conversion and their tests.
2. Pass-through outputs in `#[node]`, with adapter tests.
3. Shared filters as `#[cube]` functions in `node/filter.rs`. Mitchell–Netravali
   (any B, C) and Lanczos-3 are implemented and tested on the host, CPU, WGPU
   and Vulkan; box and triangle remain.
4. `RgbField`, Interpolate and Sample with identity chains: delivers Resample.
   Test constant preservation, identity within tolerance, odd extents,
   signed/HDR samples, zone-plate aliasing and preview/export geometry on CPU
   and WGPU.
5. Projective stage and Perspective Transform: crop, angle, quad, validation,
   borders, auto-crop; GUI handles separately.
6. Radial stages and Lens correction with per-channel TCA, after deciding how a
   field locates the optical centre (capture geometry versus `full`, and
   Orientation).
7. `MonoField` with the first mask consumer.

## Open questions

- Border policy and auto-crop default.
- Default filter: Lanczos-3 and Catmull–Rom ring around bright highlights in
  scene-linear HDR (signed halos); Mitchell or area may be the safer default.
- EWA versus the scaled-filter footprint, after measuring extreme keystones.
- Lens correction coordinates: capture geometry, orientation and crop.
- Promoting `Chain` to a payload so one map applies to an image and its mask.
- Deferred nonlinear value operations (above).
- A citable derivation of cumulative loss under repeated resampling, to state
  the single-filter benefit quantitatively; not yet researched.

## References

[1] P. S. Heckbert, "Fundamentals of texture mapping and image warping," M.S.
thesis, Univ. of California, Berkeley, Tech. Rep. UCB/CSD-89-516, Jun. 1989.
[Online]. Available: <https://www2.eecs.berkeley.edu/Pubs/TechRpts/1989/5504.html>

[2] D. P. Mitchell and A. N. Netravali, "Reconstruction filters in computer
graphics," *Comput. Graph. (Proc. SIGGRAPH)*, vol. 22, no. 4, pp. 221–228,
Aug. 1988, doi: 10.1145/378456.378514.

[3] N. Greene and P. S. Heckbert, "Creating raster Omnimax images from multiple
perspective views using the elliptical weighted average filter," *IEEE Comput.
Graph. Appl.*, vol. 6, no. 6, pp. 21–27, Jun. 1986,
doi: 10.1109/MCG.1986.276738.

[4] OpenImageIO contributors, `imagebufalgo_xform.cpp` (`filtered_sample`,
`warp`) and `imagebufalgo.h`, commit ee1c64138da35671d4985e313274fe058e5e38e3.
[Online]. Available:
<https://github.com/AcademySoftwareFoundation/OpenImageIO/blob/ee1c64138da35671d4985e313274fe058e5e38e3/src/libOpenImageIO/imagebufalgo_xform.cpp>

[5] E. Bruneton and F. Neyret, "A survey of nonlinear prefiltering methods for
efficient and accurate surface shading," *IEEE Trans. Vis. Comput. Graph.*,
vol. 18, no. 2, pp. 242–260, Feb. 2012, doi: 10.1109/TVCG.2011.81.

[6] M. Pharr, B. Wronski, M. Salvi and M. Fajardo, "Filtering after shading
with stochastic texture filtering," *Proc. ACM Comput. Graph. Interact.
Tech.*, vol. 7, no. 1, art. 1, May 2024, doi: 10.1145/3651293.

[7] Tracel AI, "Kernel fusion," *The Burn Book*. [Online]. Available:
<https://burn.dev/books/burn/performance/good-practices/kernel-fusion.html>

[8] Foundry, "How nodes concatenate," *Nuke User Guide*. [Online]. Available:
<https://learn.foundry.com/NUKE/content/comp_environment/transforming_elements/node_concatenation.html>

[9] Blender Foundation, "Realtime compositor," *Blender 3.6 Manual*. [Online].
Available: <https://docs.blender.org/manual/en/3.6/compositing/realtime_compositor.html>

[10] Blender Foundation, "Realtime Compositor: Immediately realize
transformations," pull request #112332. [Online]. Available:
<https://projects.blender.org/blender/blender/pulls/112332>

[11] C. Elliott, "Functional images," in *The Fun of Programming*, J. Gibbons
and O. de Moor, Eds. Basingstoke, U.K.: Palgrave Macmillan, 2003, ch. 7.
Images as functions over continuous space.

[D1] darktable developers, `src/iop/ashift.c`, `src/common/interpolation.c`,
`src/iop/iop_api.h`, `src/develop/pixelpipe.h`, commit
e43fbf3f7323775de78739257722d686c2d9da79. [Online]. Available:
<https://github.com/darktable-org/darktable/tree/e43fbf3f7323775de78739257722d686c2d9da79/src>

[V1] J. Hanika et al., vkdt, `src/pipe/modules/{crop,lens,resize}`,
`src/pipe/connector.h`, commit f0be6383fbf80bc58d58b991ded28bf3da01c1c3.
[Online]. Available:
<https://github.com/hanatos/vkdt/tree/f0be6383fbf80bc58d58b991ded28bf3da01c1c3/src/pipe>

[R1] RawTherapee developers, `rtengine/iptransform.cc` and
`rtengine/simpleprocess.cc`, commit 5f486d3678b34c74ba0c63571c17babe20935019.
[Online]. Available:
<https://github.com/RawTherapee/RawTherapee/tree/5f486d3678b34c74ba0c63571c17babe20935019/rtengine>

[L1] Lensfun contributors, "Lens corrections," *Lensfun Manual*. [Online].
Available: <https://lensfun.github.io/manual/latest/corrections.html>
