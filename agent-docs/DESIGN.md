# Design

The decisions behind Drip and the requirements they answer, one entry each.
[ARCHITECTURE](ARCHITECTURE.md) describes what the code does, and the
module docs state the rules. Unfinished work is in [TODO](TODO.md),
measurements in [PROGRESS](PROGRESS.md).

Status: **Requirement** is stated by the user, **Decided** is agreed, and
**Tentative** and **Open** are unsettled and may be challenged. Dates mark
decisions from the compute-model migration and later.

## Scope

- **Requirement:** The root README addresses users: CubeCL computation,
  freely arranged controls and views, colour-managed output and pipeline
  checks. Explain only the unusual canvas gestures. Demo captures use the
  Sony ILCE-7RM3 fixture, show only Drip and appear inline with a visible
  caption, which identifies gamut warnings. Use cheerful, benefit-led copy.
- **Decided:** The next milestone is a packaged, user-testable proof of
  concept with scene-illuminant white balance and orientation. Other
  image-quality and editing limits are accepted, and broader reference
  validation and SpyderX measurements do not block it. Verify installation,
  launch and a RAW-to-TIFF smoke test.
- **Decided:** “Scientific” means sound, documented image processing, not
  calibrated radiometry or ACES conformance. Quantitative sensor and chart
  calibration is outside this milestone.
- **Requirement:** Correct display colour on Wayland, macOS and Windows.
  Linux is the primary platform. X11 is unsupported.
- **Decided:** Prototype project formats have no migration guarantee. The
  format version is the `drip` crate's major version, and other versions are
  rejected.

## Model

### Evaluation

- **Decided:** Evaluation is lazy, after Haskell. Each check depends only on
  what its event supplies: an edit checks ports, admission checks
  parameters, and evaluation runs contracts, requirements and file reads,
  reporting failures on the node. Edit-time contracts had made the same edit
  pass or fail depending on whether a RAW was loaded.
- **Decided:** Only views and actions force evaluation, as `main` forces
  thunks. A preview targets the nodes with a view, and export runs when its
  action runs.
- **Decided (2026-10-13):** A node that reads input files is a foreign node,
  stamped (path, length, modification time) whenever it is keyed, so a
  changed file is read again. Files are assumed not to change while read.
  This replaces the source form, which read its files when first forced and
  kept them until chosen again. Assets, the files a kernel's parameters
  name, are still read on first use and kept with the node, failure
  included.
- **Decided:** One sequential queue with one evaluator runs previews, export
  and auto exposure. Edits read no files, and opening a project reads only
  its JSON. A demand is a graph snapshot forced under one `GlobalContext`,
  with an optional region per target. Actions use full detail. Demands are
  not merged, and a later demand reuses what the store kept. A GUI action may
  return parameters, applied as an ordinary edit.
- **Decided (2026-10-13):** A preview does not wait for foreign work. It
  evaluates under `Policy::Defer`: waiting views keep what they showed, the
  status line names the job's node without an error, and the worker sends
  the latest preview again when the job ends. Actions wait. The GUI's
  evaluators keep a disk tier, so reopening a project reads its RAWs.
- **Decided:** A worker serves one project. New and Open replace it, and the
  old thread finishes its current command, skips its queue and loses its
  receiver, so reused node IDs cannot pick up old results. New and Open are
  disabled while an action runs, since it cannot be interrupted and its
  result would be lost.

### Payloads and ports

- **Decided:** One `Rgb` payload, with its interpretation in a typed
  description. Float identity is exact, so an ICC profile near Rec.2020 is a
  different space.
- **Decided (2026-10-13):** `RgbDesc` keeps `extent`, `full` and
  `window` instead of embedding a `Lattice`, so stored descriptions and the
  disk format stay as they are. `Rgb::lattice` and `Rgb::on` convert.
- **Decided:** `Payload { NAME, Desc, Host, validate, check, allocate }`,
  with `DeviceLayout` for payloads a kernel reads. Structured data stays on
  the CPU, and nodes flatten the numbers a kernel needs into
  per-invocation constants.
- **Decided (2026-10-13):** Residency is not part of a node signature. The
  consumer asks for host or device samples, which the compute model needs
  before a planner or store can place materializations. This replaces the
  `Cpu<P>`/`Device<P>` placements and [TYPE_SYSTEM](TYPE_SYSTEM.md)'s
  placement known at connect time.
- **Decided:** No scale or encoding descriptor until a producer differs.
  `RgbDesc::full` records the full-detail extent, so contracts derive
  preview geometry instead of reconstructing it from `extent · scale`, which
  is inexact after `div_ceil`. Applying gains to balanced data composes the
  balance. `Display` impls supply every interpretation label.
- **Decided (2026-10-13):** A raster holding part of its image states its
  `window` in its description, and `extent` keeps counting stored samples,
  so every kernel that sizes its launch by `extent` stays correct. A window
  covering the whole lattice is rejected, so equal samples have one
  description. Only frontends and actions receive windows.

### Nodes

- **Decided:** An unconnected optional input is incomplete wiring. A node
  that needs it in some mode, such as CAT16 As shot, reports it at
  evaluation.
- **Decided:** Views and actions are kernels with an empty body. A map's
  contract describes its output like a kernel's, so both share one
  mechanism. A foreign node has no contract, since its outputs'
  descriptions exist only once it has run.
- **Decided (2026-10-13):** `Forward` outputs share the raster a node holds
  under a new description. Published rasters are never written again, so
  sharing keeps nodes pure.
- **Decided:** Files a node names besides its inputs, such as the output ICC
  profiles of Export TIFF and Preview proofing, are assets.
- **Decided:** Headless actions are declared with the node, and a kind has
  at most one action, a GUI extra or the node's. TIFF export is such a node,
  so projects carry no separate export target. The CLI is a scaffold until
  batch work.
- **Decided:** RAW outputs the mosaic and facts-only metadata, and consumers
  derive what they apply. A user matrix becomes an optional port when chart
  calibration needs it.
- **Decided:** One generated `static Kind` per kind, compared by identity.
  Frontend extras bind through the generated typed `Inputs` and `Typed`.
- **Decided (2026-10-13):** A node file names only the node-facing API:
  `ports`, `payload`, `runtime`, `body`, `lattice`, `expand::SubGraph`,
  `definition::Assets`, the action values type and `registry::tag`. Bodies,
  warps and lattices are that API by design, so they left `shape.rs`, which
  keeps what only evaluation reads.
- **Decided:** Kinds and tags are listed by hand, without link-time
  registration: explicit lists are readable and avoid linker-section
  portability risks.

### Parameters and GUI bindings

- **Decided:** One typed declaration per parameter field is the source of
  `Default` and the schema, and a const assertion checks that they agree.
  File roles live in the schema, and dialogs use its label and extensions.
- **Decided:** GUI extras are `impl GuiNode`, listed like kinds. Kinds
  without one use the schema controls. Test-only kinds and bindings live in
  `test_nodes`.

## Processing

- **Requirement:** Combine crop, rotation and perspective in Perspective
  Transform. Resample by a factor with interpolation and antialiasing.
- **Decided:** JPEG export uses `jpeg-encoder` with the output profiles and
  actions of TIFF, defaulting to sRGB, quality 95 and 4:4:4. Reduced chroma
  uses area averaging, and quality and chroma subsampling stay independent
  controls. TIFF and JPEG share capture tags and omit the
  original orientation, since stored pixels carry their geometry.
- **Decided (2026-10-13):** Geometry is deferred and filtered once: warps'
  projective maps compose, and Resample chooses the filter and size where
  it materializes. One footprint-scaled stage serves interpolation and
  antialiasing ([FIELD](FIELD.md)). This replaces `RgbField` with
  Interpolate and Sample.
- **Decided (2026-10-12):** Perspective Transform's geometry is one ordered
  four-point quad plus an output aspect, in bijection with projective maps,
  so crop and rotation need no parameters of their own
  ([FIELD](FIELD.md#perspective-transform)).
- **Decided (2026-10-12):** Points mapped outside the source are black, and
  edge taps repeat the edge. Auto-crop is a GUI constraint (Keep inside) plus
  a Fit action, not a stored mode. Resample defaults to Lanczos-3, and
  Perspective Transform resamples with it unless a Resample follows,
  accepting signed halos around HDR highlights
  ([FIELD](FIELD.md#borders-and-auto-crop)).
- **Decided (2026-10-12):** Perspective Transform has its own editing view
  without proofing, whose Frame, Level and Perspective tools edit the same
  quad. A view's window may edit its node ([FIELD](FIELD.md#editing-view)).
- **Open:** Lens-correction coordinates and single-channel geometry for
  masks ([FIELD](FIELD.md#open-questions)). The compute model proposes masks
  as scalar images instead of a `MonoField` payload.
- **Tentative:** The node order in [TODO](TODO.md#next-image-nodes) serves
  ordinary still-photo editing first and reuses Exposure, CAT16 and Sigmoid.
  Temperature/tint and neutral sampling extend the existing white-balance
  interface. It does not change the proof-of-concept gate.
- **Decided:** RAW metadata holds common capture, lens, GPS and sensor
  fields and native calibration. Derived values belong to consumers, and
  missing calibration hides neither metadata nor mosaic. Maker notes are out
  of scope.
- **Decided:** Bayer site gains update samples and clipping levels in one
  node, with both outputs in the same coordinates. Capture facts stay
  immutable and do not describe a transformed image's geometry.
- **Decided:** Keep as-shot gains through highlights and demosaic, then fold
  reference over as-shot gain ratios into the camera matrix, after
  darktable's “as shot to reference” placement (THIRD_PARTY \[22\]). CAT16
  derives the scene white from the mapped camera neutral or a custom xy and
  adapts to D65 in linear Rec.2020 before rendering.
- **Decided:** Full CAT16 adaptation, composed into one working-RGB matrix,
  needs no scene luminance or surround. Partial adaptation needs a policy:
  generalized von Kries keeps inverse and composition consistency [7], WGM
  models incomplete adaptation differently [8], and high-chroma experiments
  expose limits in all six tested CATs [9]. Model choice stays separate from
  illuminant estimation, and temperature/tint is a white-point UI, not a
  CAT. CAM16/Hellwig and mixed illuminants are separate [4], [5].
- **Decided:** EXIF orientation travels in capture metadata, so contracts
  know the oriented extent before allocation. LibRaw's convention is decoded
  at the RAW boundary, where unknown means identity. Orient once after
  camera conversion, and keep sensor coordinates through Bayer processing.
- **Decided:** Exposure scaling is distinct from sensor-white
  normalization: neither the RAW denominator nor Sigmoid's 0.18 pivot
  establishes ACES's 18 % reflector scale [6]. Auto exposure meters linear
  Rec.2020 mean luminance and sets EV to `log2(0.18 / mean)`, a starting
  point, not calibration.
- **Decided:** Signed and HDR coordinates are valid before rendering [3].
  Sigmoid is a rendering operation although its output stays
  identity-encoded. LittleCMS handles output conversion with the selected
  intent. ACES target-aware rendering and gamut compression are references,
  not prototype requirements [1], [2]. The working space stays Rec.2020/D65.
- **Decided:** Missing camera calibration is a supported-input limitation,
  not a reason to invent a matrix. Absent calibration stays distinct from
  extraction failures (Sigma DNGs have ColorMatrix tags but zero LibRaw
  `cam_xyz`).
- **Decided:** TIFF depth is u8, u16 (default) or f32. LittleCMS converts into
  the output encoding before quantization, and depth selects no transfer
  function.
- **Tentative:** Explain the scene-adjustment → rendering → encoding order in
  help and templates, without an ordering check.
- **Requirement:** Sensor calibration needs matching sites, CFA phase and
  sample units, so decoding is separable from black subtraction,
  normalization and crop, or keeps enough to account for them. Spatial gains
  may need saturation and noise data.
- **Requirement:** Chart calibration produces a reusable transform with
  stated basis, normalization, reference illumination and fit residuals.
  Chart and subject may differ in grid and exposure.
- **Requirement:** Masks, gain fields and scalar samples have distinct
  contracts despite shared storage, and their range, broadcasting, division
  and geometry rules belong to consumers. Masking after filtering differs
  from restricting the filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate.
  Processing updates or discards metadata whose assumptions it changes.
- **Tentative:** Lens correction needs lens identification, crop factor and
  profiles, and vignetting may need focus distance rather than minimum focus
  distance. Correct before orientation or account for transformed
  coordinates. Distortion and lateral CA are per-channel coordinate maps,
  and vignetting is a per-pixel gain.
- **Decided:** Multiple-frame operations consume collections of decoded
  values, with no frame-stack abstraction in the graph.

## Compute

- **Decided (2026-10-13):** The compiler embedded in evaluation is explicit,
  as [COMPUTE_MODEL](COMPUTE_MODEL.md#proposal-for-drip) proposes: nodes
  declare a shape, evaluation fuses segments between materialization
  points, derives regions and preview scale from shapes, and memoizes in one
  store. All eight migration steps landed on 2026-10-13.
- **Decided (2026-10-13):** The deferred steps share one entry point,
  `Image::step::<P>(Step, desc)`, and the compiler one,
  `Plan::extend(&Step, from, to)`, instead of a method per shape on each. A
  plan's front is an enum, resampling or a stencil, so the composition rules
  are one match and invalid combinations do not exist.
- **Decided (2026-10-13):** Maps after a fan-out each recompute the shared
  plan instead of cutting the segment. This trades arithmetic for bandwidth
  and keeps results independent of evaluation order.
- **Decided (2026-10-13):** A warp returns projective maps in full-detail
  pixels, and its contract describes ordinary `Rgb` on the output lattice,
  so a domain origin folds into the maps and no field payload exists.
  Filters register as bodies, so the kernel names no node code.
- **Decided (2026-10-13):** A step composes only where the result is exact,
  so users never see the grouping. The compiler does not special-case
  linear maps. The rules are in `plan.rs`.
- **Decided (2026-10-13):** The stage kind and filter are comptime, so a
  plan without geometry compiles to the map-only kernel. An interpolating
  filter at factor one reproduces a reindex bit for bit, which
  Orientation's tests check with Lanczos-3 and Box.
- **Decided (2026-10-13):** The fused kernel is one `#[cube(launch)]` shell
  whose comptime argument is the body sequence. Bodies keep the expression
  order of the kernels they replaced, so results are bit-identical. Fusion
  took the colour tail from 6.6 ms to 1.8 ms at full detail.
- **Decided (2026-10-13):** The store replaces the requirement of no
  persistent intermediate cache. Outputs are keyed by content, and the kind
  ID determines a map's body, so input keys cover a plan's bodies and
  constants without hashing them. `version` is bumped by hand when a
  kernel's numerics change. Each evaluator starts empty.
- **Decided (2026-10-13):** Durability orders eviction, and tags state it.
  `io` is High. `input-transform`, `bayer-raw`, `demosaic`, `recovery`,
  `geometry` and `color` are Medium, and `look`, `output-transform`,
  `tonemap`, `view` and `scope` are Low. No node declares its own.
- **Decided (2026-10-13):** The store keeps rasters only, charged at the
  payload's `byte_len` per side. A deferred image no consumer materialized
  is rebuilt by its nodes' host functions, so the store never holds a plan's
  input outside the budget. A run holds its own live values, so a value the
  budget evicts mid-run stays available to its consumers.
- **Decided (2026-10-13):** A foreign node runs at scale 1, and previews
  reduce its outputs downstream. A failed job stays with its key, so a
  deferred preview does not rerun a failing decode in a loop.
- **Decided (2026-10-13):** A demand has one scale and a region per target,
  the smaller API, since every frontend demand has one scale. Only a node
  without outputs takes a region, since a node computed from part of its
  inputs would publish part of its outputs under a whole-image key.
- **Decided (2026-10-13):** Regions propagate backward inside fused
  segments, and a window equals the same samples of the whole image bit for
  bit. A crop of a stored whole raster is copied in one launch: kernels read
  dense buffers, and a strided view would need every kernel to take
  strides.
- **Open (2026-10-13):** Kernel halos. An opaque kernel running on its
  output region plus a declared halo needs that region before it runs, but
  warps downstream describe their maps only when their host functions run.
  It needs lazily run kernels or a describing pass, and Bayer windows
  aligned to CFA cells. RCD therefore runs whole once at scale 1, and pans
  recompute only the tail.
- **Decided (2026-10-13):** Scale dependence derives from shapes. Nodes
  that do not read the scale receive scale 1, so a key without the scale
  hides no dependence on it.
- **Decided (2026-10-13):** The preview reduction is compiler work, with
  one claimed exception: a `scaled` kernel produces its own output, as the
  demosaics reduce each CFA plane, which no box filter on RGB matches. A
  consumer with a scaled shape filters full-detail input onto its reduced
  lattice in one stage. A warp's output is still checked, since Orientation
  keeps a full-detail input's sampling. Footprints clamp at
  `MAX_FOOTPRINT`, so reductions beyond 16 alias. This replaces demosaic
  reducing for the whole graph.
- **Decided (2026-10-13):** A stencil's host function returns its radius,
  as a warp returns its maps, because Sharpen's radius is a parameter. A
  crop grows by the reach through the earlier segment, which settles halos
  for stencils without lazy kernels: a 1600 × 1000 crop through the tail and
  Sharpen writes 3 226 100 samples in 1.01 ms, the whole image 84 779 520 in
  24.77 ms.
- **Decided (2026-10-13):** A stencil's input always materializes, since
  fusing maps into its taps would recompute them per tap. The planned
  `linear` declaration is therefore not implemented. Stencils read only
  `MapPayload`s, because a Bayer stencil needs windows aligned to CFA cells.
- **Decided (2026-10-13):** Lengths in parameters are full-detail pixels.
  Rounding a stencil's reach up to one sample keeps the body's weights, so
  full detail is never approximated and a preview shows less of the effect.
- **Decided (2026-10-13):** `Border::Repeat` or `Border::Zero` answers
  positions outside the lattice, so a window equals the whole output there.
  Sharpen repeats edges, where darktable leaves a border unsharpened.
- **Decided (2026-10-13):** Sharpen is a per-channel unsharp mask in linear
  RGB after darktable's sharpen: radius 0.25 to 4 full-detail pixels, reach
  2.5 radii, amount −1 to 4. It has no threshold, since a threshold in
  linear light has no fixed meaning across exposures. Its 2D window costs
  14 ms over the tail at full detail with reach 5.
- **Tentative (2026-10-13):** A meta node adds nodes with their kinds'
  generated `add` to the sub-graph's `Dag`, instead of the planned
  `SubGraph::add(&Kind, params)`. That keeps parameters and port payloads
  typed and adds no second graph builder.
- **Tentative (2026-10-13):** A meta node expands when it runs, after its
  inputs resolve, because expansion reads input descriptions that exist only
  once upstream nodes have run. Expanding before keying would need a
  describing pass that foreign nodes cannot join.
- **Decided (2026-10-13):** The store memoizes the 64 expansions used last
  by the meta node's key. Equal input keys mean equal descriptions. Failed
  expansions are not kept.
- **Decided (2026-10-13):** Sub-graphs hold no foreign node, whose job the
  store owns under its own key, and no meta node, so expansions do not
  nest. A sub-graph node reads the meta node's inputs reduced to the
  requested scale.
- **Decided (2026-10-13):** Pyramid blur is the first meta node: Box
  halvings, Sharpen at −1 per level and a Mitchell Resample back to the
  input's full extent. It needed an exact Resample size, which may change
  the aspect ratio. Blending the blur with its input needs a map over two
  images ([TODO](TODO.md)).
- **Decided (2026-10-13):** `Policy::Wait` serves headless evaluation,
  actions and tests. `Policy::Defer` leaves a running job running, and a
  demand while it runs takes the same job.
- **Open (2026-10-13):** Cancelling a job no demand needs, through a
  cooperative flag. RAW decode (one LibRaw call) and Hot pixel removal
  (milliseconds) cannot use one, so none exists yet.
- **Decided (2026-10-13):** The disk tier keeps `foreign = disk` outputs
  across evaluators and processes. Entries are immutable and may be deleted
  at any time. `Evaluator::new` has no disk tier, so tests never write the
  user's cache.
- **Tentative (2026-10-13):** Disk entries carry no checksum. A renamed file
  is complete unless the system crashed, and a damaged length or
  description fails to read, but flipped sample bits go undetected. A
  checksum would read every sample twice per reopen. Add one if damaged
  entries appear.
- **Decided (2026-10-13):** RAW decode is `foreign = disk`, versioned by
  `drip_raw::VERSION`, which is bumped with LibRaw, its build, the shim and
  Drip's normalization. A new evaluator reads the Sony decode from disk in
  89 ms instead of decoding in 398 ms.
- **Decided (2026-10-13):** Hot pixel removal is a `foreign = memory` node.
  The template does not include it.
- **Decided:** CubeCL computation with one kernel source per algorithm
  across runtimes. wgpu presentation and colour management stay
  frontend-owned. `DRIP_BACKEND` selects explicitly and never falls back.
- **Requirement:** Do not distribute the CPU runtime, whose LLVM JIT is too
  large. Distribution uses Vulkan SPIR-V on Linux, native Metal on macOS and
  WGSL/D3D12 on Windows, and xtask checks the target. These are candidates,
  not measured winners.
- **Decided:** RCD scratch is allocated before its first pass and released
  after its last dispatch. CubeCL owns queued bindings, so no host waits.
- **Decided (2026-10-13):** Reduce and bins bodies are declared statics, as
  filters are, because they have several parts and sizes the signature does
  not state. Reduce bodies fold in place over `&mut [f32]`, since an
  accumulator type per node would need a generic kernel per accumulator.
  Bin bodies receive the pixel's position, which waveforms need.
- **Decided (2026-10-13):** `plan.rs` owns the reduction rules once. Global
  count copies are capped at 2²⁰ counts: a level-1 waveform took 6.1 ms in 32
  copies spread over 25 MB and 2.0 ms in five, within the RTX 3080's L2
  cache. Bodies compare uploaded f32 bin edges for exact exposure
  boundaries, and the vectorscope normalizes RGB before XYZ for HDR
  headroom.
- **Decided (2026-10-13):** A reduction reads a deferred input in place
  only when it is the input's sole consumer, and otherwise the input
  materializes once: three scopes took 5.03 ms recomputing the tail and 4.19
  ms reading it once. Maps after a fan-out still recompute, since
  materializing would cut their segment.
- **Decided (2026-10-13):** A frontend consumes a reduce or bins target's
  outputs, demanded like any value. A reduction's shape is not scaled, since
  its output has no lattice, and its key carries the scale through its
  input's.
- **Decided (2026-10-13):** Histogram, Waveform and Vectorscope stay GUI
  kinds and are bins nodes whose bin bodies are drip-gui's only device code.
  A view running `Image::count` would have kept the old binding but recounted
  an unchanged image on every preview, where the store now serves the
  counts.
- **Decided (2026-10-13):** Auto exposure is `node::exposure::meter`, so a
  headless frontend can meter too, with the exact Y = 0.18 target.
- **Decided:** Softproof, gamut check and preview packing stay on the CPU.
  Packing uses Rayon over disjoint RGBA f16 slices.
- **Decided (2026-10-13):** Export TIFF writes an image over the device
  budget in bands of rows, equal byte for byte to an export in one piece.
  Whole kernels upstream, RCD first, still allocate whole images, and the
  TIFF encoder takes the converted image in one buffer, because tiff 0.11.3's
  `write_strip` does not set the writer's compression. JPEG binds its input
  whole.

## Performance

- **Requirement:** Apply mechanical simplifications first. Ownership,
  scheduling and contract changes are separate work.
- **Decided:** GUI bindings stay frontend-owned. Canvas layout and drawing
  share a node snapshot per pass. The worker keeps packed preview images
  until released, not their source pixels.
- **Tentative:** Presentation-only edits need current drawing settings or
  re-preparation, since suppressing evaluation alone leaves prepared
  settings stale. Typed single-output evaluation stays public API.
- **Tentative:** Preview levels (powers of two) stay in the frontend. The
  core accepts any integer scale and validates it at evaluation.
- **Decided:** Render tests use production packing, upload, binding and
  pipeline setup, with expected colours independent of that code.
- **Decided:** The GPU display test initializes the compute runtime before
  its own wgpu instance. CubeCL serializes compute startup, which prevents
  concurrent first Vulkan calls that crash the NVIDIA ICD, so the test needs
  the selected `DRIP_BACKEND`.
- **Requirement:** Do not optimize prematurely. Tracing adds no
  synchronization. Benchmark artifacts stay outside Git.
- **Decided:** Sub-300 ms latency is acceptable without further
  optimization.
- **Decided:** Benchmark protocol: Sony fixture (7968 × 5320), release
  builds, wgpu on the RTX 3080, `RAYON_NUM_THREADS=12`, serial runs, the
  real GUI worker with Preview, the three scopes and Export. Warm up levels
  2, 1 and 0, repeat them five times, alternate ±0.1 EV five times at level
  1, and issue five identical level-1 requests. Report medians and ranges,
  excluding drawing, upload and file writing. Harness and results live in
  `../drip-benchmark-results/`.
- **Tentative (2026-10-13):** The ±0.1 EV alternation hits the store from
  its third sample, so measurements add five level-1 requests with a new EV
  each.

## GUI

- **Requirement:** Preserve universal node controls and views, per-node
  pop-out windows and interaction logic. Internal APIs may change.
- **Decided:** Display labels are sentence case and independent of
  persisted IDs, which name the operation in kebab case, qualified where
  unclear (`rcd-demosaic`, `export-tiff`), without migration aliases.
- **Decided:** Tags come from one list, `drip::registry::TAGS`, named by
  path so a misspelling fails to compile. Frontends declare none, so `view`
  and `scope` live there too. Tags say what a node is for, since ports
  already say what it accepts. By convention a node takes at most one ACES
  stage: input
  transform, look or output transform. RAW decoding and chromatic adaptation
  count as input transforms, and Exposure is a look. `io` marks file
  readers and writers. `bayer-raw` leaves room for X-Trans and monochrome
  tags.
- **Decided:** A palette adds nodes: an `egui::Modal` sized to the window,
  opened by right-clicking the background or Space over the canvas (the node
  goes under the pointer), or from the menu (the centre of the visible
  canvas). Search ranks name prefixes, then word prefixes, then other
  matches of name, ID and tags, and tag chips narrow to kinds carrying every
  chosen tag. Each result shows its tags and its help's first sentence.
  Filtering by a dragged wire's payload would read ports, not tags.
- **Decided:** Choice variants own their dependent parameters, and only the
  selected variant's fields are shown and saved.
- **Decided:** Projects use `.drip` and templates `.drip-template`. Opening
  a template creates an untitled project, and saving a template resets its
  exposed parameters to their defaults.
- **Decided:** Menu buttons and shortcuts share one command table, matching
  modifiers exactly in every window: New Ctrl+N, Open Ctrl+O, Save Ctrl+S,
  Save as Ctrl+Shift+S, Save template Ctrl+Alt+S, Undo Ctrl+Z, Redo
  Ctrl+Shift+Z (Cmd on macOS). Button tooltips show the shortcuts, and
  focused text fields keep their keys.
- **Decided:** Undo restores whole-graph snapshots of the DAG, names,
  template inputs and layout, not the preview level, selection or windows.
  Nodes are shared `Arc`s, so a replaced node and its loaded file stay in
  memory while a step refers to it. A step ends when the pointer is released
  in the window that edited. Text fields edit once on losing focus, and
  action results are steps of their own. `DRIP_HISTORY` sets the depth, 100
  by default.
- **Decided:** New and Open ask to save, discard or cancel unsaved edits in
  every window. A failed or canceled save keeps the project.
- **Decided:** Ports show payload and endpoint names, optional inputs are
  marked `?`, and long titles are elided with the full name on hover.
  Unsaved edits are tracked apart from evaluation requests.
- **Decided:** Scope overlays keep axes and compact colour-space labels and
  nothing else. Waveforms default to logarithmic density, histograms to
  linear.
- **Decided:** Text uses vendored Source Sans 3 at weight 400, names at 600,
  and Source Code Pro, embedded in the binary. egui's fonts are disabled, so
  glyphs outside these render as `?`: no emoji, and node header buttons use
  `≡` (parameters) and `❒` (view). Every test harness installs the fonts, so
  text is measured as shipped.
- **Decided:** Inspector help joins Rustdoc source wraps and keeps
  paragraphs. Reflow belongs to presentation.
- **Decided (2026-10-12):** Built-in templates are `.drip-template` files in
  `templates/`, embedded and listed by hand, loaded through the same path as
  a user's template. New creates the first, Neutral restore. A test requires
  each to equal its re-serialization, so format changes regenerate them, and
  they carry the crate's major version without migration. They contain GUI
  view kinds, so a headless frontend cannot load them as they are, and the
  core keeps only a test fixture of the RAW chain.
- **Decided (2026-10-13):** The image view navigates in the full-detail
  plane, so 100 % shows one full-detail pixel per display pixel at every
  preview level. Magnified to 100 % or more past its preview, the window
  asks for a full-detail crop of its visible region, once per region and
  preview generation, evaluated after any waiting preview. The crop is
  drawn over the preview while both belong to the same graph, so an edit
  shows the new preview alone until its crop arrives. Scopes keep the
  preview's samples. Fit, smaller zooms and full-detail previews ask for
  nothing.
- **Decided:** Neutral restore uses a landscape layout: views share a top
  edge, processing staggers down through Orientation, and RAW and TIFF sit at
  the bottom corners.
- **Tentative:** Rounded orthogonal wires could help backward links, but
  routing around nodes needs obstacle-aware paths. No router chosen.

## Distribution

- **Decided:** Headless CI builds `drip/cpu` and tests with
  `DRIP_BACKEND=cpu`, and GPU tests are opt-in. Tests on every platform gate
  distribution jobs, which only build and upload. Validate test changes with
  `drip/cpu` too, since default builds omit CPU-only tests.
- **Decided:** Worker tests allow 90 seconds for compute completion, since
  CPU tests share a compute client with full-RAW evaluations.
  Synchronization gates keep five seconds.
- **Requirement:** Minimize user-installed dependencies. Native processing
  libraries are static, and the normal Linux C/C++ runtime may stay
  external.
- **Decided:** Vendor unmodified sources at release tags, and never fetch
  during builds. Pins and licenses are in [THIRD_PARTY](../THIRD_PARTY.md).
  LibRaw uses its shared source manifest through `cc` on Windows too.
- **Decided:** Preserve user compiler flags and targets. Defer CPU variants
  and OpenMP until measured gains justify their packaging cost.
- **Decided:** Build each target natively on the newest standard GitHub
  runners.
- **Tentative:** Linux `.tar.xz`/Flatpak, unsigned macOS `.app` ZIPs and
  Windows ZIPs.

## Documentation

- **Decided (2026-10-13):** Each documentation level answers one question
  and points down. The crate index gives the parts and one example, a
  module doc says what the module owns and why, and states its rules when
  it implements them (`plan.rs`, `store.rs`, `eval/mod.rs`), and an item doc
  states its contract. This file records decisions, and ARCHITECTURE gives
  the overview. The drip README's examples compile as doc tests through a
  `#[cfg(doctest)]` include in `lib.rs`.

## References

[1] Academy of Motion Picture Arts and Sciences, “Overview of Output Transform
Structure,” ACES Documentation, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/system-components/output-transforms/technical-details/rendering-overview/>.
Accessed: Oct. 10, 2026.

[2] Academy of Motion Picture Arts and Sciences, “Reference Gamut Compression,”
ACES Documentation, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/rgc/overview/>. Accessed: Oct. 10, 2026.

[3] Academy of Motion Picture Arts and Sciences, “ACEScg — A Working Space for CGI
Render and Compositing,” ACES Documentation, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/encodings/acescg/>. Accessed: Oct. 10, 2026.

[4] C. Li et al., “Comprehensive color solutions: CAM16, CAT16, and CAM16-UCS,”
Color Research & Application, vol. 42, no. 6, pp. 703–718, 2017,
doi: <https://doi.org/10.1002/col.22131>.

[5] L. Hellwig and M. D. Fairchild, “Brightness, lightness, colorfulness, and
chroma in CIECAM02 and CAM16,” Color Research & Application, vol. 47, no. 5,
pp. 1083–1095, 2022, doi: <https://doi.org/10.1002/col.22792>.

[6] S. Tobenkin, “IDTs: How Do They Work,” ACESCentral, Apr. 15, 2020.
[Online]. Available:
<https://acescentral.com/knowledge-base-2/idts-how-do-they-work/>.
Accessed: Oct. 10, 2026.

[7] C. Gao et al., “The development of the CIECAM16 and visualization of its
domain and range,” Color Research & Application, 2025,
doi: <https://doi.org/10.1002/col.22959>.

[8] C. Shen and M. D. Fairchild, “Weighted Geometric Mean (WGM) method: A new
chromatic adaptation model,” PLOS ONE, vol. 18, no. 8, e0290017, 2023,
doi: <https://doi.org/10.1371/journal.pone.0290017>.

[9] Y. Shen et al., “Performance of Chromatic Adaptation Transforms for Stimuli
at Different Chroma Levels,” Color Research & Application, vol. 51, no. 4,
e70108, 2026, doi: <https://doi.org/10.1002/col.70108>.
