# Modelling computation: research note

2026-10-12. Research for one question: what if the compiler that Drip,
darktable and vkdt each embed implicitly became explicit, so a data structure
describes the DAG of computations and evaluation compiles it into few, large
kernels? Everything here was **Open** in [DESIGN](DESIGN.md) terms. The
existing design docs were read as drafts to question, not as settled facts.
The [proposal](#proposal-for-drip) at the end was adopted on 2026-10-13.

Sources: Drip `aec5201` (`continous`); vkdt `f0be638` and darktable
`ddc0bc5`, both cloned and read at those commits; CubeCL 0.11.0 sources from
the local registry; aces-core, Blender, Apple, OCIO, Halide and the papers
cited below. No code, measurements, dependencies or kernels were changed.

## The problem

Every DAG image editor contains three compilers, usually unnamed:

| Layer | Compiles | Drip | darktable | vkdt |
|---|---|---|---|---|
| Kernel | per-pixel math to device code | CubeCL `#[cube]` | C, then OpenCL C again | GLSL compute |
| Glue | a node declaration to ports, contracts, allocation, launches, transfers | `#[node]`, plus a hand-written contract and launch sequence | `iop_api.h`: dozens of optional callbacks per module | `connectors`, `params`, `create_nodes`, `modify_roi_in/out`, `check_params` |
| Schedule | DAG plus request to order, regions, residency, reuse | `Evaluator`, `GlobalContext::scale` | pixelpipe with ROI, tiling and a hashed cache | `graph-run-*` with ROI markers and run flags |

The cost of a node is the part of the glue and schedule layers that cannot be
derived because the node body is opaque. Measured on the clones:

| | Count |
|---|---|
| darktable modules (`src/iop/*.c`) | 97 |
| of which carry a second OpenCL body (`process_cl`) | 77 |
| with a hand-written tiling callback | 36 |
| with hand-written `modify_roi_in` | 22 |
| with hand-written point distortion | 16 |
| that handle the preview scale themselves (`roi->scale`, `piece->iscale`) | 58 |
| OpenCL kernel files, total lines | 46, 20 710 |
| `sigmoid.c` + `sigmoid.cl` lines, one algorithm | 982 + 297 |
| Drip RCD: launches and scratch buffers arranged by hand in one node | 5, 6 |

The user's framing is right: a DAG of lazily forced pure nodes, a few
effects at the edges, and the open question is how much each node must say
about itself. The answer this note arrives at: the assumption *is* a
declared **shape**. A node that says only "I am a kernel" gets placement and
lifetimes. A node that says "I am a point operation", "a stencil of radius
r", "a warp by map t", "a fold with monoid m", or "a sub-graph built from
these" gets fusion, regions, preview scale, contracts, cache keys and most of
its GUI derived. The compiler is worth building exactly to the degree that
real pipelines fit a small set of shapes. The inventory below says they do.

## What an application needs

vkdt ships about a hundred modules, darktable 97, and ACES 2.0 defines the
output side. Grouped by shape:

| Shape | Body signature | vkdt | darktable | ACES 2.0 |
|---|---|---|---|---|
| Point (map) | `(consts, px...) -> px`, optionally with coordinates | exposure, colour, filmcurv, grade, curves, colenc, zones, OpenDRT, reinhard, vignette, grain, check, okgmtclp | exposure, colorbalancergb, filmic, sigmoid, colorin/out, channelmixer, basecurve | the whole output transform: clamp, RGB→JMh, tonescale on J, chroma compression on M, gamut compression, JMh→RGB, display encoding [22], [23] |
| Point with precomputed tables | as above, plus `params -> tables` on the host | colour (thin-plate spline fit), filmsim | sigmoid primaries, colorbalancergb | `init_ODTParams`: 360-entry hue tables for reach M, gamut cusps and upper-hull gamma from peak luminance and limiting primaries [22] |
| Stencil (fixed footprint) | `(consts, window<r>) -> px` | demosaic passes, usm, hotpx, ca, deconv iterations | demosaic, sharpen, hotpixels, highlights passes | none |
| Multi-scale (sub-graph of stencils and resamplers) | `(params, desc) -> graph` | llap (up to 12 levels, 2 + 2·levels nodes from `create_nodes`), denoise, eq, contrast, hilite, dehaze, inpaint, guided, wavelet | diffuse, denoiseprofile, highlights (laplacian), retouch, atrous, toneequal | none |
| Warp (gather at a map) | `map: Point -> Point` plus Jacobian; one sampler | crop, lens, resize, align, mipmap | clipping, ashift, lens, liquify, finalscale | none |
| Reduction / scatter | `fold` with a monoid or bins | hist, rawhist, ciediag, pick, autoexp, loss | histogram, pickers, auto exposure | none |
| Host solve | small CPU code producing constants | colour's TPS solve, lens fit, mv2rot | ashift fit, colour calibration | none |
| Multi-frame / feedback | frame arrays, feedback connectors | align, hdrmerge, accum, blend, svgf | none | none |
| Neural / external | opaque, expensive | jddcnn, kpn, oidn, dmlp | none | none |
| IO | source/sink | i-raw, o-jpg, ... | rawspeed/libraw, export | none |
| Raster / geometry | draw calls, ray tracing | draw, overlay, bvh, rt | drawn masks | none |

Two facts follow. First, the colour pipeline is point operations with small
tables: ACES's normative CTL is a chain of per-pixel functions whose only
state is tables built from parameters [22]; darktable's scene-referred
chain and vkdt's grading modules are the same. Fusing point chains removes
nearly every intermediate buffer in that section. Second, the spatial
operations are few but large (demosaic, denoise, local contrast, lens), and
they are the ones that need regions, halos and scale handling. Those are
what a shape-aware compiler derives and what darktable and vkdt write by
hand per module.

## Prior art that compiles the graph

- **Core Image.** A `CIImage` is "an image recipe"; nothing renders until
  asked; "the calculation path begins at the destination and works its way
  back to the source pixels", which "minimizes the number of pixels used in
  any calculation" (ROI and domain of definition) [1]. The runtime
  "minimizes the number of temporary registers (per kernel) and temporary
  pixel buffers (per filter graph)" [1]; the patents describe reducing the
  number of programs needed to render a graph [3]. Kernels come in three
  classes, colour (point), warp and general with an ROI callback [2]. This
  is the user's idea, shipped since 2005: a shape taxonomy in the API and
  concatenation by the runtime.
- **OpenColorIO 2.** A processor is a list of ops; the GPU path emits one
  shader function from the whole list, LUTs as textures, dynamic properties
  as uniforms; "generic" mode processes "all the ops as-is, without baking,
  like the CPU renderer" [4]. Ops are data; two renderers consume one list.
  ACES 2.0 lands in OCIO 2.4 as fixed-function ops for exactly the stages
  above [24]. Scope: point operations only, which is the ACES scope.
- **Blender realtime compositor.** The evaluator walks the schedule and
  accumulates consecutive "pixel nodes" into a "pixel compile unit" while
  they agree on single-valuedness and domain; any non-pixel node ends the
  unit, which is compiled into one operation [5]. Each pixel node supplies
  a GPU shader-node and a CPU multi-function, so the fusion machinery is
  shared but bodies are still written twice. Transforms are deferred and
  realized on a domain, which is Drip's field idea at compositor scale.
- **Halide.** Pure functions over infinite integer domains plus a separate
  schedule; bounds inference derives every region; stencils fuse through
  tiling with recomputation; JIT or AOT; backends CPU, CUDA, OpenCL, Metal,
  D3D12, Vulkan (beta), WebGPU (beta); LLVM 21–23 required [6]–[8].
  Deployed on "hundreds of millions of phones" [7]; Google's Pixel Visual
  Core and Adobe Photoshop are its reported users. This is the complete answer
  to "compile the pipeline including stencils", bought with C++ and LLVM,
  hand schedules on GPU, and seconds of JIT per distinct pipeline.
- **CubeCL and Burn.** `#[cube]` rewrites a Rust function into a function
  that builds IR when called; comptime values specialize kernels at first
  compile and variants are cached [9]. Burn's fusion traces element-wise,
  reduction and matmul groups and "compiles it into a single kernel" at
  runtime on any CubeCL runtime [10]. So runtime kernel construction exists
  in Drip's stack today; what is missing is a graph language above it.
- **tinygrad, XLA, torch.compile.** Lazy graphs; element-wise ops fuse into
  their consuming reduction; movement ops are views composed without copies
  [31]. The view composition is the warp chain in `RgbFieldDesc`.
- **libvips and GEGL.** Demand-driven region evaluation on the CPU: a sink
  pulls tiles, each thread computes a tile through the whole pipeline with
  four lock operations per tile [20]; GEGL classes operations as point
  filter, area filter with a declared halo, composer, source, sink and meta
  [19]. A shape taxonomy with regions gives streaming and bounded memory
  for huge images. GEGL still duplicates some ops in OpenCL.
- **Nuke Blink.** One kernel source "converted into device-specific code
  for each target", C++ or SIMD on the CPU, OpenCL or CUDA on the GPU [21].
  Kernels declare per-pixel versus ranged versus random access, which is a
  shape declaration the runtime uses for bounds.
- **Salsa, Adapton, build systems à la carte.** Memoized demand-driven pure
  functions with revisions, durability levels and early cutoff (a recomputed
  value equal to the old one stops invalidation) [17], [18], [38]. This is
  the Haskell laziness in DESIGN with the memoization that `main`-forces-
  thunks implies but Drip omitted.
- **Meganeura (2026).** A compact ML compiler on Vulkan and Metal through
  wgpu. It first built naga IR directly, hit "arena handles,
  expression-emission ranges" and errors "without a useful source location",
  then moved to generated WGSL text: 949 Rust lines plus 1 402 WGSL lines
  replaced 6 359 lines of IR construction, kept because "every generated
  kernel [is] independently inspectable" [27]. Evidence for emitting text,
  not IR, if a custom compiler is built.
- **Functional images, two-level embeddings, comonadic stencils.** Elliott's
  images are functions of continuous position [35]; Pan compiles such
  expressions to C by running the program to build a syntax tree [14];
  Accelerate splits scalar expressions (`Exp`) from array combinators
  (`Acc`) and fuses by combinator laws [13]; finally-tagless interpreters
  let one body be evaluated, compiled and printed [15]; Ypnos gives
  stencils a comonadic signature with declared extents [16]; Dex types
  index sets [40]. These are the algebra for the shapes below.

## Questioning the designs

### Drip

- The `RgbField` chain is a hand-rolled compiler for one shape: warps
  compose as `contramap`, and `Sample` is the materialization point. The
  same deferral applies to point operations (compose as `fmap`) and, with
  tiling, to stencils. FIELD.md's open item "deferred nonlinear value
  operations" is the general fusion question in disguise.
- Contracts are free-form Rust per node. Exposure and Sigmoid copy their
  description; Sample and RCD compute extents. With shapes, a point
  operation's contract is derived (same lattice; colour from the declared
  `Rgb<In> -> Rgb<Out>`), a stencil's from its halo, a warp's from its map
  and domain, a reduction's from its monoid.
- `GlobalContext::scale` is a global integer applied inside demosaic. It
  gives previews but no regions: a 100 % view needs a full-detail crop, and
  export of a 100-megapixel RGB at f32 is 1.2 GiB per buffer, so three live
  buffers break the 4 GiB rule. darktable and vkdt both have ROI; Core
  Image, Halide and libvips derive it. Bounds inference over shapes is the
  mechanical fix.
- `Cpu<P>` and `Device<P>` as port types couple node signatures to
  residency. In a compiled model residency is a schedule decision: kernels
  consume device buffers, host values are constants, and the compiler
  decides where an edge lives. Keep the typed descriptors; drop placement
  from the declaration.
- `Write` outputs are allocated before the body runs. Inside a fused chain
  they do not exist. Allocation belongs to materialization, not to nodes.
- "No persistent intermediate cache" (DESIGN, Performance) was set before
  the 2026-10-08 benchmark showed sigmoid-only edits regressing from 359 to
  402 ms at half detail because nothing upstream was reused, while full
  recompute on the GPU was 6.8× faster than Rayon. A bounded,
  content-addressed cache with durability (next sections) keeps purity and
  fixes the regression; the requirement should become a byte budget.
- Scopes live in the GUI with their own CubeCL kernels. They are reductions
  and belong in the same language as everything else.
- TYPE_SYSTEM's "kernel inputs and outputs have concrete Rust types known at
  compile time, not abstract algebraic structures" is half right. Storage
  must be concrete at materialization points. Bodies can be written against
  `Window`, `Point` and `Pixel` interfaces, and must be if they are to be
  fused; the abstraction lives at the kernel boundary, bytes live at buffer
  boundaries.
- "Demands are not merged" (DESIGN, Evaluation): with memo keys, merging is
  the default and costs nothing.
- The comparison in PROGRESS that Drip "checks substantially more" than
  vkdt is true and cheap. The checks that matter, radius units at preview
  scale, halo coverage, lattice and phase agreement for Bayer stencils, are
  only possible for a compiler that knows shapes.
- The uncommitted `COMPUTATION.md` in the `t3code` worktree is superseded by
  this note and should not be merged.

### darktable

- A linear module stack, not a DAG; branching happens inside each module
  through blend and mask machinery that every module carries.
- Two or three bodies per algorithm (77 of 97 with OpenCL, some settings
  CPU-only, new modules CPU-first), with a runtime fallback to the CPU on
  OpenCL errors and a manual asking users to report CPU/GPU differences.
  This is the maintainability cost in its purest form.
- Three pipelines (full, preview, preview2) at different scales; 58 modules
  read the scale themselves. Preview and export agreement is each author's
  responsibility, which is where the known class of mismatch bugs lives.
- Each distorting module resamples on its own; `distort_transform` maps
  points and masks but does not concatenate resampling, so a lens-corrected,
  levelled, cropped image is interpolated three times.
- The cache is good: one hash per module of the stack up to it including
  ROI, kept in bounded cache lines [26]. It proves prefix reuse pays.
- Colour typing is a seven-value enum plus profile info, with conversions
  inserted by the pipe.

### vkdt

- GPU-only, Vulkan-only, GLSL compute, no CPU path. "One source" is solved
  by fiat and works on Linux, Windows and macOS through MoltenVK. The cost
  is driver sensitivity and no fallback at all.
- Module versus node is the right split: the module is what the user sees,
  `create_nodes` expands it into a sub-DAG of kernel launches with regions
  (llap builds its pyramid in C, demosaic inserts a resize node when the
  requested scale allows half-size). Drip has no such layer, which is why
  RCD arranges five launches by hand and llap would be thirty.
- Types are tokens: `rgba:f16`, `*`, `&input`. Colour meaning travels in a
  flat `dt_image_params_t` with primaries and TRC enums copied per module
  and mutated along the way. Checks are minimal.
- Regions: forward full sizes from sources, backward requests from sinks,
  soft and hard markers to resolve conflicts, and each module choosing its
  scale policy (`denoise` demands full resolution, `demosaic` halves when
  the request is coarse enough). Same mismatch class as darktable, held
  consistent by one author.
- No intermediate cache and no fusion: every node is a dispatch through f16
  images. A thirty-node chain on a 24-megapixel f16 RGBA image moves on the
  order of 10 GiB per frame, fine on a discrete GPU, not on an iGPU. That is
  why Core Image, OCIO and Blender fuse.
- Feedback connectors, video, ray tracing and neural modules show the node
  model is general; none are needed for a stills prototype.

## An explicit model

### Two levels

A **scalar language** for bodies and a **graph language** for shapes, as in
Accelerate's `Exp`/`Acc` [13] and Halide's funcs versus schedules [6].

- Bodies are plain functions over `Pixel`, `Window<r>`, `Point`, constants
  and tables. They never see bytes, extents or launch geometry. Written in
  whatever kernel language the route below picks; one source.
- Shapes are data: `Map(body)`, `Stencil{radius, border, body}`,
  `Warp{map, jacobian}`, `Reduce{monoid, body}`, `Opaque{passes, halo}`,
  `Meta(expand)`, `Source`, `Sink`. A node declaration is a shape plus a
  parameter schema plus a colour/lattice signature.
- The graph is an expression tree over shapes. Evaluation compiles the
  demanded subtree into segments and runs them.

Haskell typeclass thinking fits this exactly and stops exactly here. The
body is written once against an interface and interpreted three ways: build
device IR or text, run natively on the host for tests and references, and
print or hash for cache keys (finally tagless [15]). Buffers are concrete
and appear only where a segment starts or ends; the descriptor there fixes
the byte layout. Nothing abstract ever reaches a launch.

### Shapes and laws

The shapes are the functor-like structures of a continuous image
`I = Point -> Pixel` [35], and their laws are the fusion rules:

| Shape | Structure | Law used for fusion |
|---|---|---|
| Map | `fmap f` | `fmap f ∘ fmap g = fmap (f ∘ g)`: a chain of point ops is one kernel |
| Warp | `contramap t` | `contramap s ∘ contramap t = contramap (t ∘ s)`: warps compose into one map and one resampling, which Drip's `Chain` already does |
| Map after warp | `fmap f ∘ contramap t = contramap t ∘ fmap f` | exact for the continuous image; discretizing it as sample-then-map is exact only for linear `f` (FIELD.md), which is the "filtering after shading" approximation every editor makes |
| Stencil | `extend k` with a declared extent [16] | fusing two stencils needs a tile with halo `r1 + r2` and recomputation, Halide's trade-off [6]; without a Halide-class scheduler, materialize between stencils |
| Reduce | `fold m` | a fusion barrier; the map feeding it fuses into the partial-sum pass |
| Meta | a function to a graph | expanded before compilation, never a runtime object |

What does and does not fuse is then a one-page decision table rather than
per-node scheduling code. The Blender rule, "keep adding pixel nodes while
single-valuedness and domain agree, any other node closes the segment" [5],
is a correct minimum.

### Materialization and segments

A segment is a maximal fusable chain between materialization points. Each
materialization point has a descriptor (lattice, colour, precision) and an
owner. Points are forced by: a stencil or opaque input, a reduction, a
fan-out that would otherwise recompute an expensive chain, a view or
action, a cache checkpoint, or an instruction-count limit (Blender splits
on hardware limits). Deciding points is the whole scheduler; it is small.

### Regions and scale

A request is a region of an output lattice at a scale. Shapes give bounds
inference for free: a map needs the same region, a stencil the region
grown by `r` in lattice units, a warp the image of the region under `t`
grown by the filter support, a reduction everything, an opaque node what
its declared halo says. Scale is a property of the lattice relative to the
full-detail plane (Drip's `full` extent already states it), so a stencil
radius or a lens coefficient declared in plane units is converted once, by
the compiler. A stencil whose radius falls below one sample at a coarse
scale is reported, not silently different. This replaces 58 modules' worth
of hand-written scale handling and the preview-versus-export class of
bugs with one rule, and removes Drip's special case of demosaic reducing
internally: demosaic declares its full-resolution output and the compiler
inserts the reduction where the request needs it, as vkdt's `create_nodes`
does by hand.

### Types

- Colour: canonical spaces as static types on pixels (`Rgb<Rec2020>`),
  camera and profile-defined spaces as runtime descriptors compared exactly,
  as Drip does now. A map body declares `Rgb<A> -> Rgb<B>`; the compiler
  checks edges when the graph is built, before any evaluation, because
  shapes make output descriptors computable from input descriptors without
  data. Drip's edit-time contract problem (pass or fail depending on whether
  a RAW was loaded) disappears for every shape but sources.
- Lattice: extent, phase (Bayer, X-Trans or none), relation to the plane.
  A stencil over a Bayer lattice declares phase-dependent windows, which
  is what RCD's passes do implicitly.
- Units: parameters that are lengths or coefficients carry a unit so the
  scale rule applies. This is the one place the schema needs a new field.
- Dependent types (Lean, Idris, Dex [40]) would make extents and phases
  part of the type. In Rust, const generics cannot carry runtime extents;
  keep extents in descriptors with checks at graph build. Use the idea, not
  the tool.

### Cache

A content-addressed memo over the pure DAG [17], [18]:

- Key: hash of (node kind and version, canonical parameters, shape program
  of the segment, request region and scale, input keys). Sources key on
  file identity plus size and modification time, with a full content hash
  when cheap enough.
- Durability: sources and decode outputs high, structural and expensive
  parameters medium, look sliders low. The scheduler places checkpoints at
  durability boundaries (after decode, after demosaic, after denoise), so a
  slider edit recomputes only the fused tail. Early cutoff applies to small
  values (matrices, tables) by value hash.
- Tiers: device-resident within a byte budget (the 4 GiB rule becomes the
  budget), host memory, and disk for nodes flagged expensive: RAW decode
  output, neural denoise, frame stacking, lens fits. One facility, one key
  space, so an export after an interactive session reuses the checkpoints
  the preview made at full scale if the budget kept them.
- Invalidation by key only. A changed file is a new key, which also fixes
  the "choose the file again" workaround.

### Effects

Sources, sinks, external solvers and GUI feedback stay at the boundary:
views are sinks demanding a display-sized region, actions are sinks
demanding full regions, pickers and auto-exposure are reductions whose
results become parameter edits (vkdt's `ui_callback`), editing views are
sinks plus parameter writes. Nothing in the model changes for them.

## Implementation routes

| Route | Bodies in | Backends | Fusion mechanism | Host tests | Dependencies | Risks |
|---|---|---|---|---|---|---|
| R1. Keep Rust + CubeCL, add shapes | Rust `#[cube]` | CUDA, HIP, Metal, Vulkan SPIR-V, WGSL, CPU LLVM | comptime-specialized fused kernel (segment's op list as a comptime value, bodies dispatched by index) or direct `cubecl-ir::Scope` building as Burn does [9], [10] | CPU runtime in `cargo test` | already present | alpha API churn; Rust-subset quirks in bodies (the `swap` workaround in Sigmoid); comptime argument types need checking; no bounds inference provided |
| R2. Rust + generated shader text + wgpu | WGSL (or GLSL) snippets with a declared shape | Vulkan, Metal, D3D12, WebGPU through naga [30] | the compiler emits one shader per segment; naga validates; wgpu `naga-ir` or WGSL source | none native; lavapipe or SwiftShader in CI | wgpu only | you own a small compiler (point and warp fusion, bounds); no CPU reference path; two languages (Rust and WGSL), one kernel source |
| R3. Rust + Slang | Slang modules with interfaces and generics [29] | SPIR-V, Metal, CUDA, CPU via slang-llvm [28] | emit a Slang program per segment, compile at runtime, reflect | CPU target (needs slang-llvm) | large C++ compiler binary; unofficial Rust bindings | dependency size and build; Metal lacks f64, `[unroll]`, wave intrinsics [28]; runtime compile latency |
| R4. Halide core | Halide C++ funcs | CPU, CUDA, OpenCL, Metal, D3D12, Vulkan beta [8] | Halide: bounds inference and stencil fusion for free | Halide CPU JIT | LLVM 21–23, C++ core, Rust GUI over FFI | GPU schedules by hand or weak autoschedule; seconds of JIT per graph structure (cache by structure, parameters as `Param`); Vulkan beta |
| R5. IREE / MLIR | custom dialect or linalg | CPU, Vulkan, Metal, CUDA, ROCm [33] | industrial compiler | yes | very large | ML-centric; a dialect is a research project |
| R6. rust-gpu | Rust compiled to SPIR-V | SPIR-V only (Metal through MoltenVK or naga translation) | ahead-of-time only, so an in-shader op interpreter for point chains | the same Rust runs natively | nightly toolchain pin [32] | maturity; no runtime specialization |
| R7. Others | Futhark (no Metal or Vulkan [12]), Accelerate (NVIDIA only [13]), Taichi (Python, drifting [36]), Mojo (Linux and macOS, Windows announced [34]), Lean (no device ecosystem), Python (prototyping only) | | | | | not viable for a desktop app on three platforms today |

Notes on the mechanisms:

- **In-shader interpreter.** One generic point kernel reading an op list
  from a uniform buffer and switching per op. No runtime compilation, no
  compile cache, works with ahead-of-time routes (R6). Uniform branches are
  cheap; register pressure and instruction count bound the segment length.
  Worth a measurement before any code generator: for a 30-op colour chain
  it may be within noise of a specialized kernel.
- **Comptime specialization.** The same kernel with the op list as a
  comptime value unrolls at expansion and CubeCL caches the variant [9].
  Zero code generator; one new kernel per distinct segment; compile
  latency on structure changes, none on parameter edits if parameters are
  runtime constants.
- **Text generation.** Meganeura's experience says emit text, keep each
  generated kernel inspectable, and let the shader compiler own invariants
  [27]. Blender and OCIO do the same. Build IR directly only with a
  ready-made builder such as `cubecl-ir::Scope`.
- **Stencil fusion.** Only Halide gives it for free. Every other route
  materializes between stencils. Given the inventory, that is acceptable:
  the pyramids are sub-graphs whose intermediates are small, and the big
  stencils (demosaic, denoise) are checkpoints anyway.

## Device assumptions

- Assume a SPIR-V-class compute device: f32 arithmetic, optional f16
  storage, workgroup shared memory, 2D buffers or images, subgroup
  operations optional and detected. That is what naga guarantees across
  Vulkan, Metal and D3D12 and what CubeCL's wgpu runtimes expose.
- "Always Vulkan" (vkdt's choice) buys bindless descriptor indexing, push
  descriptors and buffer device addresses at the cost of MoltenVK on macOS.
  wgpu gives native Metal; the marginal features are not needed by the
  shapes above. Vulkan-only is not recommended; SPIR-V-class is.
- Do not build or ship a CPU backend. DESIGN already forbids distributing
  CubeCL's LLVM runtime. Use lavapipe or SwiftShader for CI and headless
  checks, and the host interpretation of bodies (finally tagless, or
  CubeCL's CPU runtime in R1) for numerical references.
- Precision per edge (f16 intermediates as in vkdt) is a schedule decision
  the descriptor can carry later; not a design element now.
- Determinism: no fast-math contraction in fused kernels unless both
  fused and unfused variants are tested against the host reference; fusion
  changes rounding only through reassociation and contraction.

## Recommendation

Regardless of route, adopt the model: shapes as data, bodies against
interfaces, segments at materialization points, bounds inference and scale
from shapes, a content-addressed cache with durability and a disk tier,
effects at the boundary. The user's intuition that "every pure computation
can be something" is right; the something is its shape.

Route ranking for a throwaway prototype whose goal is minimal code per node:

1. **R1 first, as an experiment.** It needs no new dependency and answers
   the two open empirical questions: whether comptime-specialized or
   IR-built fusion of a point chain beats separate launches enough to
   matter at 42 megapixels, and whether `#[cube]` bodies against `Window`
   and `Pixel` traits stay readable. If CubeCL's churn or Rust-subset
   limits bite, the shape declarations and compiler carry over to R2.
2. **R2 if the kernel language should be a shader language.** Smaller
   dependency surface than R3, text generation as the literature advises,
   and the Rust side stays the single place for types, shapes and the
   cache. Accept no native CPU execution.
3. **R4 only if stencil fusion and bounds inference are worth a C++ core.**
   It is the most complete compiler available; it moves the project's
   centre of gravity to C++ and LLVM.

Falsifiable steps, in order, each a day or two:

1. Write the 17 current kinds plus a local-laplacian pyramid and a wavelet
   denoise as shape declarations on paper. Count the `Opaque` ones. If more
   than demosaic and the neural placeholders are opaque, the shape set is
   wrong.
2. Fuse Exposure → camera matrix → CAT16 → Sigmoid into one kernel by
   comptime op list, and separately by the in-shader interpreter. Measure
   against four launches on the Sony fixture at full and half detail. Keep
   the benchmark protocol in DESIGN.
3. Add region requests: a 100 % crop of the preview must evaluate only its
   footprint through stencils of declared radius.
4. Add the memo cache with durability. A Sigmoid-only edit must recompute
   only the fused tail; a RAW re-open must hit the disk tier.
5. Only then decide whether R1 stays or R2 replaces it.

## Proposal for Drip

Added 2026-10-13 after the user asked how to put the model into this
project, including foreign and expensive computations. Adopted on
2026-10-13 ([DESIGN](DESIGN.md#compute)). [ARCHITECTURE](ARCHITECTURE.md)
describes the steps that landed.

### Decisions

1. **A value is a computation with optional storage.** `Image` is the port
   value, possibly deferred. `Raster` is a materialized `Image`: samples on
   a `Lattice` with a host and a device representation. Residency leaves
   the node signature, the consumer's shape decides where a materialization
   lives, and the store transfers lazily.
2. **A node is a shape plus a body.** `#[node]` keeps the kind, schema,
   help, tags, registry and project format and adds `shape`. Existing nodes
   start as `kernel` and adopt shapes one by one.

   | Shape | Body | What the compiler derives |
   |---|---|---|
   | `map` | `#[cube]` function of one pixel | fusion into the segment's kernel, region identity, scale invariance |
   | `stencil(radius, border)` | `#[cube]` function of a window | halo, region growth, radius converted per scale |
   | `warp` | host function returning a map, its Jacobian and domain | composition with neighbouring warps, one resampling at the next materialization |
   | `resample(filter, size)` | none | explicit materialization, replacing Sample and Interpolate |
   | `reduce(monoid)` | `#[cube]` accumulator plus combine | partial sums, readback, a constant output for nodes and the GUI |
   | `kernel(halo \| whole)` | a function with explicit launches | inputs and output materialized, region by halo, cache key |
   | `meta` | host function returning a subgraph | expansion before compilation (pyramids, iterations) |
   | `foreign(cache = disk \| memory)` | host function on a worker thread | host inputs, always memoized, full detail only, cancellable |
   | `source` / `sink` | as now | file identity in the key, sinks demand regions |

3. **Evaluation compiles.** A demand is targets plus region plus scale.
   Evaluation expands `meta`, computes types along edges, cuts the subgraph
   into segments at materialization points, keys each segment and runs only
   the uncached ones. A segment of maps and warps is one fused kernel.
4. **One store, three tiers, one key space.** Keys are Merkle hashes of
   kind and version, canonical parameters, segment program, region, scale
   and input keys. The device tier has a byte budget. Durability separates
   sources and foreign outputs, structural parameters and look parameters.
   The store is a memo, never a project artifact.
5. **Foreign work runs at full detail and is memoized on disk.** Reopening a
   project skips LibRaw. Previews reduce the cached full-detail output, and
   a pending foreign value yields "pending" to views.
6. **Regions and scale are compiler work.** A 100 % view requests a crop,
   export tiles by budget, and preview scale inserts one resampling at the
   first materialization finer than requested. Demosaic's internal reduction
   stays as an optimization its kernel may claim.
7. **Types narrow rather than restart.** `Type { lattice, pixel }` with
   `Meaning` = `Rgb(Coordinates) | Scalar(Quantity) | Mask | Levels |
   Metadata`. Shapes compute output types, so edges are checked at connect
   time for every node but a source. Masks and gain maps are scalar images.
8. **Names.** `Image`, `Raster`, `Lattice`, `Warp`, `Resample` and `Plan`.
   Not "field" and not "functional".

Each earlier model identified values wrongly (cached results, typed buffers,
placed buffers). This one identifies a value with its computation, so later
changes add shapes without changing what a value is.

### Migration in the `drip` crate

| Step | Work | Status |
|---|---|---|
| 1 | `Image`, `Raster`, `Store` and `Shape`. Every node a kernel or source, no placement in signatures. | Landed 2026-10-13 |
| 2 | `map` and the fused segment kernel. Exposure, camera matrix, CAT16 and Sigmoid as maps, measured against four launches. | Landed 2026-10-13 |
| 3 | `warp` and `resample` replace `RgbField`, Interpolate, Perspective Transform and Sample. Orientation becomes an exact warp. | Landed 2026-10-13 |
| 4 | Keys, durability and the device budget. A Sigmoid-only edit recomputes only the tail. | Landed 2026-10-13 |
| 5 | `foreign` with the disk tier. RAW decode moves to it, and one stacking or model placeholder proves the path. | Landed 2026-10-13, with Hot pixel removal as the second node |
| 6 | Regions: a 100 % crop view and tiled export. | Landed 2026-10-13, with kernels whole and no halos |
| 7 | `reduce`. Scopes and auto exposure leave GUI-owned kernels. | Landed 2026-10-13, with a `bins` shape beside `reduce` |
| 8 | `stencil` and `meta` with the first sharpen and pyramid nodes. | Next |

## References

[1] Apple Inc., "Core Image concepts," *Core Image Programming Guide*
(archive). [Online]. Available:
<https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/CoreImaging/ci_advanced_concepts/ci.advanced_concepts.html>

[2] Apple Inc., "Creating custom filters," *Core Image Programming Guide*
(archive). [Online]. Available:
<https://developer.apple.com/library/archive/documentation/GraphicsImaging/Conceptual/CoreImaging/ci_custom_filters/ci_custom_filters.html>

[3] Apple Inc. (assignee), "System for reducing the number of programs
necessary to render an image," U.S. Patent 7,231,632.
[Online]. Available:
<https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7231632>

[4] OpenColorIO contributors, "Shaders," *OpenColorIO API documentation*.
[Online]. Available: <https://opencolorio.readthedocs.io/en/main/api/shaders.html>

[5] Blender Foundation, `COM_node_tree_evaluator.hh` and
`COM_pixel_operation.hh`, `source/blender/compositor/`, main branch,
accessed Oct. 12, 2026. [Online]. Available:
<https://github.com/blender/blender/tree/main/source/blender/compositor>

[6] J. Ragan-Kelley, C. Barnes, A. Adams, S. Paris, F. Durand and S.
Amarasinghe, "Halide: a language and compiler for optimizing parallelism,
locality, and recomputation in image processing pipelines," in *Proc. PLDI*,
2013, pp. 519–530, doi: 10.1145/2491956.2462176.

[7] J. Ragan-Kelley et al., "Halide: decoupling algorithms from schedules
for high-performance image processing," *Commun. ACM*, vol. 61, no. 1,
pp. 106–115, Jan. 2018, doi: 10.1145/3150211.

[8] Halide contributors, *Halide README*, main branch, accessed Oct. 12,
2026. [Online]. Available: <https://github.com/halide/Halide>

[9] Tracel AI, *CubeCL README*, main branch, accessed Oct. 12, 2026.
[Online]. Available: <https://github.com/tracel-ai/cubecl>

[10] Tracel AI, *burn-cubecl-fusion README*, main branch, accessed Oct. 12,
2026. [Online]. Available:
<https://github.com/tracel-ai/burn/tree/main/crates/burn-cubecl-fusion>

[11] T. Henriksen, N. G. W. Serup, M. Elsman, F. Henglein and C. E. Oancea,
"Futhark: purely functional GPU-programming with nested parallelism and
in-place array updates," in *Proc. PLDI*, 2017, pp. 556–571,
doi: 10.1145/3062341.3062354.

[12] T. Henriksen, "OpenCL, CUDA and HIP as compilation targets for
functional array programs," FOSDEM 2025. [Online]. Available:
<https://fosdem.org/2025/events/attachments/fosdem-2025-4504-opencl-cuda-and-hip-as-compilation-targets-for-functional-array-programs/slides/237559/presentat_WLc4ywz.pdf>

[13] M. M. T. Chakravarty, G. Keller, S. Lee, T. L. McDonell and V. Grover,
"Accelerating Haskell array codes with multicore GPUs," in *Proc. DAMP*,
2011, pp. 3–14, doi: 10.1145/1926354.1926358.

[14] C. Elliott, S. Finne and O. de Moor, "Compiling embedded languages,"
*J. Funct. Program.*, vol. 13, no. 3, pp. 455–481, 2003,
doi: 10.1017/S0956796802004574.

[15] J. Carette, O. Kiselyov and C. Shan, "Finally tagless, partially
evaluated: tagless staged interpreters for simpler typed languages," *J.
Funct. Program.*, vol. 19, no. 5, pp. 509–543, 2009,
doi: 10.1017/S0956796809007205.

[16] D. A. Orchard, M. Bolingbroke and A. Mycroft, "Ypnos: declarative,
parallel structured grid programming," in *Proc. DAMP*, 2010, pp. 15–24,
doi: 10.1145/1708046.1708053.

[17] A. Mokhov, N. Mitchell and S. Peyton Jones, "Build systems à la carte,"
*Proc. ACM Program. Lang.*, vol. 2, no. ICFP, art. 79, 2018,
doi: 10.1145/3236774.

[18] Salsa contributors, "The Salsa algorithm," *Salsa book*. [Online].
Available: <https://salsa-rs.github.io/salsa/reference/algorithm.html>

[19] GEGL contributors, "GEGL operation API." [Online]. Available:
<https://gegl.org/operation-api.html>

[20] J. Cupitt, "How it works," *libvips documentation*. [Online].
Available: <https://www.libvips.org/API/8.18/how-it-works.html>

[21] Foundry, "BlinkScript," *Nuke Reference Guide*. [Online]. Available:
<https://learn.foundry.com/nuke/content/reference_guide/other_nodes/blinkscript.html>

[22] Academy of Motion Picture Arts and Sciences, `lib/Lib.Academy.OutputTransform.ctl`,
*aces-core*, accessed Oct. 12, 2026. [Online]. Available:
<https://github.com/ampas/aces-core>

[23] Academy of Motion Picture Arts and Sciences, "Rendering overview,"
*ACES Documentation*. [Online]. Available:
<https://docs.acescentral.com/system-components/output-transforms/technical-details/rendering-overview/>

[24] OpenColorIO contributors, "OCIO 2.4 release." [Online]. Available:
<https://opencolorio.readthedocs.io/en/main/releases/ocio_2_4.html>

[25] J. Hanika et al., vkdt, commit f0be6383fbf80bc58d58b991ded28bf3da01c1c3:
`src/pipe/modules/api.h`, `src/pipe/connector.h`, `src/pipe/module.h`,
`src/pipe/graph-run-modules.h`, `src/pipe/modules/{llap,demosaic,denoise}/main.c`,
`src/pipe/modules/readme.md`. [Online]. Available:
<https://github.com/hanatos/vkdt/tree/f0be6383fbf80bc58d58b991ded28bf3da01c1c3>

[26] darktable developers, darktable, commit
ddc0bc5cb679f1531761d05b8bf1a4eda4099b4d: `src/iop/iop_api.h`,
`src/develop/pixelpipe_cache.h`, `src/iop/sigmoid.c`,
`data/kernels/sigmoid.cl`. [Online]. Available:
<https://github.com/darktable-org/darktable/tree/ddc0bc5cb679f1531761d05b8bf1a4eda4099b4d>

[27] "Meganeura: portable GPU training and inference through Vulkan and
Metal," arXiv:2608.01563, Aug. 2026. [Online]. Available:
<https://arxiv.org/abs/2608.01563>

[28] Slang contributors, "Target compatibility," *Slang documentation*.
[Online]. Available:
<https://github.com/shader-slang/slang/blob/master/docs/target-compatibility.md>

[29] Y. He, K. Fatahalian and T. Foley, "Slang: language mechanisms for
extensible real-time shading systems," *ACM Trans. Graph.*, vol. 37, no. 4,
art. 141, 2018, doi: 10.1145/3197517.3201380.

[30] wgpu contributors, `ShaderSource::Naga`, *wgpu API documentation*.
[Online]. Available: <https://docs.rs/wgpu/latest/wgpu/enum.ShaderSource.html>

[31] tinygrad contributors, "Kernelize," *tinygrad documentation*.
[Online]. Available: <https://docs.tinygrad.org/developer/kernelize>

[32] Rust-GPU contributors, rust-gpu. [Online]. Available:
<https://github.com/Rust-GPU/rust-gpu>

[33] IREE contributors, "Deployment configurations," *IREE documentation*.
[Online]. Available: <https://iree.dev/guides/deployment-configurations/>

[34] Modular, "Apple silicon GPU support in Mojo" and "Timeline for Windows
support," Modular forum, 2026. [Online]. Available:
<https://forum.modular.com/t/apple-silicon-gpu-support-in-mojo/2295>,
<https://forum.modular.com/t/timeline-for-windows-support/3268>

[35] C. Elliott, "Functional images," in *The Fun of Programming*,
J. Gibbons and O. de Moor, Eds. Basingstoke, U.K.: Palgrave Macmillan, 2003,
ch. 7.

[36] AMD, *Taichi Lang documentation*, 2026. [Online]. Available:
<https://rocm.docs.amd.com/projects/taichi/en/latest/>

[37] Drip, branch `wgsl-experiment`, commit b269f2b, "record Rayon
comparison for WGSL experiment" (benchmark of 2026-10-08).

[38] M. A. Hammer, K. Y. Phang, M. Hicks and J. S. Foster, "Adapton:
composable, demand-driven incremental computation," in *Proc. PLDI*, 2014,
pp. 156–166, doi: 10.1145/2594291.2594324.

[39] Blender Foundation, `COM_shader_operation.hh` and
`COM_multi_function_procedure_operation.hh`, `source/blender/compositor/`,
main branch, accessed Oct. 12, 2026.

[40] A. Paszke et al., "Getting to the point: index sets and parallelism-
preserving autodiff for pointful array programming," *Proc. ACM Program.
Lang.*, vol. 5, no. ICFP, art. 88, 2021, doi: 10.1145/3473593.
