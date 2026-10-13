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
  view. The one exception is a field node's own editing view (Perspective
  Transform's handles), which samples through the same kernel; see
  [Editing view](#editing-view).
- **Decided (2026-10-12):** Perspective Transform's geometry is one ordered
  four-point quad plus an output aspect, in bijection with projective maps;
  crop and rotation are quads too. See [Perspective
  Transform](#perspective-transform).
- **Decided (2026-10-12):** Points outside the source are black; edits keep
  the quad inside the source by default (auto-crop). Sample defaults to
  Lanczos-3. See [Borders](#borders-and-auto-crop) and [Nodes](#nodes).
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
  undefined transforms (a quad with three collinear corners) and compute
  domains on the host before allocation.
- **Pass-through outputs (done):** a field node declares
  `output: Forward<'_, RgbField>` and sets it with `output.set(image.data)`,
  the input's samples under a new description.
  Storage is already shared and immutable, so purity holds. Nodes that only
  reinterpret (assigning RGB meaning to camera numbers) can use it too.

## Nodes

| Node | Ports | Parameters | Description change |
|------|-------|-----------|---------------------|
| Interpolate | `Rgb -> RgbField` | none | identity maps; domain = `[0, full]` |
| Perspective Transform | `RgbField -> RgbField` | quad, aspect | prepend `Projective`; domain = `[0, aspect] × [0, 1]` scaled |
| Lens correction (later) | `RgbField -> RgbField` [+ metadata] | model, TCA | prepend per-channel `Radial` |
| Sample | `RgbField -> Rgb` | filter (default Lanczos-3), size (factor or long edge) | realizes; output `full` from domain and size |

Sample sizing: full-detail output extent `N = round(domain.size · factor)` or
fit to a long edge; stored extent follows the RGB reduction rule
(`div_ceil(N, global.scale)`), so preview and export framing agree. Per output
pixel and channel the kernel evaluates the chain with forward-mode derivatives
(a point and its 2×2 Jacobian; the chain rule composes them), scales the
footprint, sums normalized taps and applies the border rule below.

Always evaluate three chains rather than branching on whether maps are shared.
Taps load one channel each instead of three channels once: equal bytes, three
weight evaluations.

Filter: **Decided** Lanczos-3 by default, as the filter users know. Its
negative lobes leave signed halos beside bright highlights in scene-linear
HDR; Sample keeps them (Rgb is signed) rather than clamping, and Mitchell
remains selectable. Minifying by `k` costs `(6k)²` taps per channel and pixel;
a separable path for axis-aligned chains is a later optimization.

### Borders and auto-crop

**Decided (2026-10-12):**

- Taps outside the stored samples repeat the nearest edge sample, so filters
  near the edge neither darken nor ring against an implied black.
- An output point whose mapped centre falls outside the source rectangle, or
  through a projective pole (`w ≤ 0`), is black (vkdt and OIIO default to
  black). Only undefined transforms are rejected. This is fixed, not a
  parameter; an alpha or coverage channel would be a payload change.
- Auto-crop is an editing constraint, not a stored mode: the output is the
  quad's content, so a convex quad inside the input's valid region shows no
  black. GUI edits keep it inside by default ("Keep inside", on), so black
  appears only when the user turns that off. Because the quad is absolute, a
  later upstream change (lens parameters) can expose black; *Fit* refits.
- *Fit*, after darktable's rotate and perspective modes [D1], keeps the
  current projective map and replaces the frame with the largest output
  rectangle inside the valid region: *original format* (default here: the
  current aspect) or *largest area* (darktable's default). darktable fits the
  centre (and aspect) with a Nelder–Mead simplex. Here, with a fixed aspect, a
  rectangle's corners are linear in its centre and scale, so *original
  format* is a linear program in three variables (maximize scale; each corner
  inside each edge's half-plane), solved exactly; *largest area* adds a
  one-dimensional search over the aspect (check that area is unimodal in it,
  or scan first).
- The valid region is the input domain mapped forward through the input's
  whole chain, so upstream lens correction is accounted for. For projective
  chains it is a convex polygon, or unbounded when the map's vanishing line
  crosses the input; Fit then has no largest rectangle and reports so. Radial
  stages need sampled boundaries and Newton inverses (step 6).

### Perspective Transform

**Decided (2026-10-12):** the parameters are an ordered quad `q_0..q_3`
(top-left, top-right, bottom-right, bottom-left of the output), normalized to
the input field's domain, and the output aspect `a = width / height`. The
output is the quad's content: the projective map `M` takes the output
rectangle's corners `(0, 0), (a, 0), (a, 1), (0, 1)` to `q_0..q_3`, scaled to
plane units. Default: the input domain's corners and aspect (identity).

- **Bijection.** Four points with no three collinear determine exactly one
  projective map of the plane taking a given four such points to them (the
  fundamental theorem of projective geometry; the map is the 8-unknown DLT
  solve) [12, Ch. 2, 4]. So ordered quads in general position and an aspect
  correspond one-to-one to (projective map, output rectangle) pairs up to
  output translation and uniform scale, which Sample's size fixes. Both sides
  have nine degrees of freedom. Rotation and crop are quads (a rotated or
  inset rectangle), so no separate angle or crop parameter exists: one would
  give several parameter sets for one result.
- **Validation.** Three collinear corners make `M` singular: rejected as
  undefined. Concave or self-intersecting quads are valid maps whose vanishing
  line crosses the output; points beyond it (`w ≤ 0`) are black. The GUI keeps
  quads convex while Keep inside is on.
- **Direction.** `M` is already output → source, the stage's direction, so the
  kernel needs no inverse; GUI handles need `M⁻¹` (closed form, 3×3).
- **Aspect.** It is the ninth degree of freedom and is stored explicitly.
  Corner edits keep it and the user sets it (*original format* or a value).
  No automatic perspective for now: estimating a photographed rectangle's true
  aspect needs the focal length, and line fitting needs structure detection.

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

## Editing view

**Implemented (2026-10-12), GUI-owned** (`drip-gui/src/node_ui/perspective.rs`).
Perspective Transform has its own view,
like Preview, so editing needs no extra Sample and Preview nodes; a downstream
Sample and Preview still show the final image. The binding's preparation
samples the node's input field with the drip Sample kernel at the preview
scale (Lanczos-3), not cropped to the quad. The canvas body shows it
with a non-interactive overlay; handles work in the pop-out window, where the
user can zoom.

**Decided (2026-10-12):** the view is separate from Preview. It shows plain
pixels with Preview's navigation and none of its proofing, profiles or
assets. It displays its input as it arrives, so a transform placed before
exposure and tone mapping shows scene-linear values clipped; templates place
geometry where the view is representative.

Two tools edit the same quad, chosen in the window's toolbar:

- **Frame** (default) shows the input mapped through the current projective
  map over the valid region's bounds (clipped to a limit when unbounded), with
  the output rectangle as an axis-aligned frame and everything outside it
  darkened. Corner and edge handles crop, dragging inside moves the frame,
  dragging outside it rotates, Fit refits; each maps the new frame back
  through `M` into the quad. A level tool takes a dragged line along a horizon
  or vertical and rotates the frame to level it.
- **Perspective** shows the input without this node's transform, with the
  quad's corners as handles. Dragging corners onto a building's edges never
  moves the image under the pointer, so there is no feedback loop through
  re-evaluation.

The prepared view carries the map and domain it was sampled with, and overlays
are drawn through those, not through the current parameters, so handles stay
on the displayed pixels while a newer evaluation is pending.
A drag moves its overlay immediately, edits parameters continuously and ends
its undo step on release.

How it is built:

- `Drawable::interact` gives a view's window the node, so handles edit
  parameters through `ControlCx::set`, as controls do.
- `image_view::show` reports where it drew the image; in editing mode the
  primary button belongs to the handles and the secondary or middle button
  pans.
- Preparation calls `sample::render` through `KernelContext::new`, twice: the
  input over its domain (Perspective tool) and the input through the current
  map over the output plane's view bounds, at most one output size beyond the
  output on each side (Frame and Level tools).
- A gesture keeps the map of the image shown when it began and converts the
  pointer through the input plane, so a preparation arriving mid-drag does
  not move its handles. Frame edits set the aspect as a ratio, keeping the
  map exactly; Keep inside bisects a move or resize at the input's edge and
  shrinks a rotation about the frame's centre. Frame handles are hidden
  ("Updating…") while the shown image predates a perspective edit.
- Not yet exercised in a desktop session; tests cover preparation on the
  compute runtime and the frame geometry.

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
   Orientation, camera conversion and their tests. Done; `RgbDesc::new` builds
   full-detail descriptions, and validation rejects storing more than `full`.
2. Pass-through outputs in `#[node]`, with adapter tests. Done (`Forward`).
3. Shared filters as `#[cube]` functions in `node/filter.rs`. Mitchell–Netravali
   (any B, C), Lanczos-3, triangle and box are implemented, with a `Filter`
   choice and `weight` dispatch, tested on the host, CPU, WGPU and Vulkan.
4. `RgbField`, Interpolate and Sample with identity chains: delivers Resample.
   Test constant preservation, identity within tolerance, odd extents,
   signed/HDR samples, zone-plate aliasing and preview/export geometry on CPU
   and WGPU. Done (`node/field/`): one kernel with a composed 3×3 map per
   channel (output index → source index), footprints from the map's
   derivative clamped to 16 source samples, exact identity for interpolating
   filters (signed zeros included), tests against an independent f64
   resampler with per-channel projective maps on CPU, WGPU and Vulkan.
5. Projective stage and Perspective Transform: quad and aspect, the DLT
   solve, validation, borders, and Fit as a host function. Then the editing
   view as a separate GUI commit series. Node done
   (`node/field/perspective.rs`): Heckbert's closed-form square-to-quad map,
   aspect from corners (mean opposite sides) or a ratio, Fit by enumerating
   the LP's vertices, `inside` for Keep inside. An integer crop through
   Sample reproduces its pixels exactly.
6. Radial stages and Lens correction with per-channel TCA, after deciding how a
   field locates the optical centre (capture geometry versus `full`, and
   Orientation).
7. `MonoField` with the first mask consumer.

## Open questions

- Automatic perspective (structure detection, fitting lines, estimating a
  rectified rectangle's aspect from focal length): not planned for now; the
  user places the quad and sets the aspect.
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

[12] R. Hartley and A. Zisserman, *Multiple View Geometry in Computer Vision*,
2nd ed. Cambridge, U.K.: Cambridge Univ. Press, 2004. Ch. 2 (projective
transformations of the plane) and ch. 4 (DLT estimation of a homography).

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
