# Continuous fields: geometry and resampling

Research reference for resampling, perspective and later lens correction,
settled with the user on 2026-10-12: the mathematics of fields, maps and
footprint-scaled filters, the Perspective Transform parameterization, border
rules and prior art. `node/field/` and `node/filter.rs` implement it with
`RgbField`, Interpolate, Perspective Transform and Sample
([ARCHITECTURE](ARCHITECTURE.md#data-model)). Step 3 of the compute model
turns geometry into deferred warps with one explicit Resample
([COMPUTE_MODEL](COMPUTE_MODEL.md#migration-in-the-drip-crate)), keeping the
mathematics here. Decisions are in [DESIGN](DESIGN.md#processing).

## Mathematics

### Fields and maps

Discrete samples `s_k` at pixel centres `p_k` and a reconstruction kernel `r`
define a continuous image [11]

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
inverse mapping needs. Lensfun's distortion and TCA models are stated in this
direction (undistorted → distorted) for the same reason [L1].

### Resampling

Sampling `F` on a new grid `y_j` with prefilter `h` is [1, ch. 4]

```text
out_{j,c} = ∫ h(y_j − y) F_c(y) dy = Σ_k s_{k,c} ρ_{j,c}(k)
```

The ideal kernel `ρ` is space-variant: it is `r` warped by `m_c` and
convolved with `h`. When the map magnifies, `r` dominates (interpolation).
When it minifies, `h` dominates (antialiasing). Both are one filter.

Drip's approximation, after OpenImageIO's `warp` [4]: one filter kernel `φ`
(box, triangle, Mitchell–Netravali [2], Lanczos-3), scaled per source axis by
the footprint from the map's Jacobian `J_c = ∂m_c/∂y` in source-pixel units,

```text
σ_x = max(1, |J_xx|, |J_xy|),   σ_y = max(1, |J_yx|, |J_yy|)
w_{j,c}(k) = φ((p_k − m_c(y_j)) ⊘ σ) / Σ_k φ(…)
```

Weights are normalized. This is isotropic per source axis and blurs strongly
anisotropic footprints (extreme keystone). Elliptical weighted averaging [3]
is the measured upgrade path, not the starting point. Plain scaling is the
special case of a constant diagonal `J`, so Resample needs no second kernel.
A separable fast path is an optimization to measure, not a design element.

### Value operations

For a per-pixel operation `g`, applying it to samples before interpolating
equals applying it to the field when `g` commutes with the weighted sum:

- **Linear and channel-preserving** (exposure, per-channel gains): exact with
  any maps.
- **Channel mixing** (camera matrix, CAT16): exact only when all channels share
  one map. With per-channel maps, `Σ_d M_cd f_d(m_c(y)) ≠ Σ_d M_cd f_d(m_d(y))`:
  mixing before a TCA correction mixes misregistered channels. Document TCA
  correction as acting on sensor-registered channels. Do not enforce an order.
- **Affine with offsets**: exact because weights sum to one.
- **Nonlinear** (sigmoid, curves, clipping): not exact. `g ∘ F` is not band
  limited, so prefiltering it needs `g` evaluated under the footprint
  (supersampling), the "filter after shading" problem [5], [6]. To first order,
  the field semantics amount to applying `g` at the higher of the two
  resolutions.

Linear operations therefore gain nothing from living inside a field: place them
before Interpolate or after Sample. Deferring nonlinear `g` needs per-pixel
bodies composed into the resampling kernel (as fused maps compose, or
Burn-style fusion [7]) and evaluated under the footprint. **Open**, revisit
with a concrete quality case.

### Sampling a field

Sample's full-detail output extent is `N = round(domain.size · factor)`, or
fits a long edge. The stored extent is `div_ceil(N, global.scale)`, so
preview and export framing agree. Per output pixel and channel the kernel
maps the sample centre through the channel's composed map, scales the
filter by the footprint, and sums normalized taps. Footprints clamp at 16
source samples per output sample, so stronger reductions alias until a
prefiltered pyramid or EWA lifts the bound. Lanczos-3 is the default: its
negative lobes leave signed halos beside bright highlights in scene-linear
HDR, which Sample keeps rather than clamping. Minifying by `k` costs `(6k)²`
taps per channel and pixel.

## Geometry

### Borders and auto-crop

- Taps outside the stored samples repeat the nearest edge sample, so filters
  near the edge neither darken nor ring against an implied black.
- An output point whose mapped centre falls outside the source rectangle, or
  through a projective pole (`w ≤ 0`), is black (vkdt and OIIO default to
  black). Only undefined transforms are rejected. This is fixed, not a
  parameter. An alpha or coverage channel would be a payload change.
- Auto-crop is an editing constraint, not a stored mode: the output is the
  quad's content, so a convex quad inside the input's valid region shows no
  black. GUI edits keep it inside by default ("Keep inside", on), so black
  appears only when the user turns that off. Because the quad is absolute, a
  later upstream change (lens parameters) can expose black, and *Fit* refits.
- *Fit*, after darktable's rotate and perspective modes [D1], keeps the
  current projective map and replaces the frame with the largest output
  rectangle inside the valid region: *original format* (default here: the
  current aspect) or *largest area* (darktable's default). darktable fits the
  centre (and aspect) with a Nelder–Mead simplex. Here, with a fixed aspect, a
  rectangle's corners are linear in its centre and scale, so *original
  format* is a linear program in three variables (maximize scale, each corner
  inside each edge's half-plane), solved exactly by trying every vertex where
  three constraints meet. *Largest area* scans aspects from 1:10 to 10:1 on a
  log scale and refines the best by golden section, since the area need not
  be unimodal in the aspect.
- The valid region is the input domain mapped forward through the input's
  whole chain, so upstream lens correction is accounted for. For projective
  chains it is a convex polygon, or unbounded when the map's vanishing line
  crosses the input. Fit then has no largest rectangle and reports so. Radial
  stages need sampled boundaries and Newton inverses.

### Perspective Transform

The parameters are an ordered quad `q_0..q_3`
(top-left, top-right, bottom-right, bottom-left of the output), normalized to
the input field's domain, and the output aspect `a = width / height`. The
output is the quad's content: the projective map `M` takes the output
rectangle's corners `(0, 0), (a, 0), (a, 1), (0, 1)` to `q_0..q_3`, scaled to
plane units. Default: the input domain's corners and aspect (identity).

- **Bijection.** Four points with no three collinear determine exactly one
  projective map of the plane taking a given four such points to them (the
  fundamental theorem of projective geometry, with the map given by the
  8-unknown DLT solve) [12, Ch. 2, 4]. So ordered quads in general position and an aspect
  correspond one-to-one to (projective map, output rectangle) pairs up to
  output translation and uniform scale, which Sample's size fixes. Both sides
  have nine degrees of freedom. Rotation and crop are quads (a rotated or
  inset rectangle), so no separate angle or crop parameter exists: one would
  give several parameter sets for one result.
- **Validation.** Three collinear corners make `M` singular: rejected as
  undefined. Concave or self-intersecting quads are valid maps whose vanishing
  line crosses the output. Points beyond it (`w ≤ 0`) are black. The GUI keeps
  quads convex while Keep inside is on.
- **Direction.** `M` is already output → source, the stage's direction, so the
  kernel needs no inverse, and GUI handles need `M⁻¹` (closed form, 3×3).
- **Aspect.** It is the ninth degree of freedom and is stored explicitly.
  Corner edits keep it and the user sets it (*original format* or a value).
  No automatic perspective for now: estimating a photographed rectangle's true
  aspect needs the focal length, and line fitting needs structure detection.

### Editing view

Perspective Transform's view (`drip-gui/src/node_ui/perspective.rs`) shows
plain pixels with Preview's navigation and none of its proofing. Two tools
edit the same quad:

- **Frame** shows the input mapped through the current projective map over
  the valid region's bounds, with the output rectangle as an axis-aligned
  frame. Corner and edge handles crop, dragging inside moves the frame,
  dragging outside rotates it, and each edit maps the new frame back through
  `M` into the quad. Level turns a dragged line level or plumb. Frame edits
  set the aspect as a ratio, which keeps the map exact.
- **Perspective** shows the input without this node's transform, with the
  quad's corners as handles. Dragging corners onto a building's edges never
  moves the image under the pointer, so there is no feedback loop through
  re-evaluation.

Overlays are drawn through the map the shown image was sampled with, and a
gesture keeps that map until it ends, so a preparation arriving mid-drag
does not move the handles.

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
  the precedent for `RgbDesc::full`. No cross-module fusion. `colour` merges
  matrix, white balance and RBF by design.
- **RawTherapee** `5f486d3` [R1]: `transformGeneral` composes lens-profile
  distortion and TCA, manual distortion, rotation, perspective and scale into
  one map, multiplies vignetting and gradient gains, and interpolates once
  (4×4 bicubic, optionally in log space). This is `sample(G · f ∘ m)` with
  linear `G`, in a fixed pipeline. Export resizes later in a separate Lanczos
  pass on L\*a\*b\* data (`simpleprocess.cc`), so transform and resize filter
  twice.
- **Nuke** [8]: consecutive transforms concatenate and filter once, and colour
  nodes break concatenation.
- **Blender** realtime compositor [9]: transform nodes record the interpolation
  method. A later node realizes. A later pull request, "Immediately realize
  transformations" [10], changed this. Its rationale was not read.
- **OpenImageIO** `ee1c641` [4]: `warp` propagates derivatives through the
  matrix and scales an isotropic filter by them.

Fixed pipelines get single filtering by construction. Drip gets it from the
graph when geometry nodes are adjacent. A discrete node between them forces a
Sample there, as a non-concatenating node does in Nuke. Help and templates
should say so. The graph does not forbid it.

## Applications

Geometry candidates beyond crop, straighten and keystone (Perspective
Transform):

- anamorphic desqueeze (non-square pixels).
- lens distortion and lateral CA, and projection changes (fisheye to rectilinear,
  cylindrical/equirectangular for panoramas).
- mesh warps (liquify), as a coarse-grid stage kind.
- registration for multi-frame work: map frame B onto frame A, then Sample on
  A's grid for HDR merge, focus stacking or burst denoise. Needs a Sample input
  that supplies the target grid.

Single-channel geometry: masks drawn in source coordinates follow the
image's geometry by passing through the same maps, and coarse calibration
grids (flat field, vignetting gain) sample onto an image grid. Reusing one
map for an image and its mask is **Open**. The compute model proposes masks
as scalar images rather than a separate `MonoField` payload. Analytic masks
with no samples are deferred until masks exist.

## Open questions

- Automatic perspective (structure detection, fitting lines, estimating a
  rectified rectangle's aspect from focal length): not planned for now. The
  user places the quad and sets the aspect.
- EWA versus the scaled-filter footprint, after measuring extreme keystones.
- Lens correction coordinates: capture geometry, orientation and crop.
- One map shared by an image and its mask.
- Deferred nonlinear value operations (above).
- A citable derivation of cumulative loss under repeated resampling, to state
  the single-filter benefit quantitatively, not yet researched.

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
