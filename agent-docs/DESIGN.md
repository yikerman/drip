# Design

The decisions behind the design and the requirements they answer.
[ARCHITECTURE](ARCHITECTURE.md) describes the design as implemented, and
`AGENTS.md` holds the code principles. Research is in
[COMPUTE_MODEL](COMPUTE_MODEL.md), [FIELD](FIELD.md) and
[TYPE_SYSTEM](TYPE_SYSTEM.md). Unfinished work is in [TODO](TODO.md),
evidence in [PROGRESS](PROGRESS.md).

Status: **Requirement** is stated by the user, **Decided** is agreed, and
**Tentative** and **Open** are unsettled and may be challenged.

## Scope

- **Requirement:** The README addresses users: highlight CubeCL computation,
  freely arranged controls/views, color-managed output and pipeline checks.
  Explain only the unusual canvas gestures and leave conventional node controls
  to the interface. Demo captures use the Sony ILCE-7RM3 fixture and show only
  Drip. Use cheerful, benefit-led product copy and show every supplied demo
  inline with its own visible caption. Identify gamut warnings in the caption.
- **Decided:** The next milestone is a packaged, user-testable proof of concept,
  including scene-illuminant white balance and orientation. Other
  image-quality and editing limitations are accepted. Broader reference
  validation and SpyderX measurements are not blockers. Verify installation,
  launch and a complete RAW-to-TIFF smoke test.
- **Decided:** “Scientific” for the prototype means sound, documented image
  processing, not calibrated radiometry or ACES conformance. Quantitative
  sensor/chart calibration is outside this milestone.
- **Requirement:** Correct display color on Wayland, macOS and Windows. Linux is
  the primary development platform. X11 is unsupported.
- **Decided:** Prototype project formats have no migration guarantee. The
  format version is the `drip` crate's major version, and other versions are
  rejected.

## Model

Settled with the user on 2026-10-11 and revised by the compute model
(2026-10-13).

### Evaluation

- **Decided:** Evaluation is lazy, after Haskell's model. Each check depends
  only on what its event supplies. A graph edit checks ports: payload types,
  occupancy and cycles. Admission checks parameters only. Contracts,
  requirements, missing inputs and file reads happen when evaluation forces
  the node and are reported on it, and downstream nodes name it. Edit-time
  contracts made the same edit pass or fail depending on whether a RAW was
  loaded.
- **Decided:** Only views and actions force evaluation, as `main` forces
  thunks. A GUI preview targets the nodes with a view. Export runs when its
  action runs.
- **Decided:** A node reads its files (source outputs or assets) when first
  forced and keeps the result, failure included. Files are assumed not to
  change, so the cache does not change the node's outputs. It holds file
  contents, not intermediate results. To pick up a changed file, choose it
  again. A source stamps the input files its parameters name (path, length,
  modification time) before reading them, and the stamps key its outputs
  (2026-10-13). Each run receives loaded host samples in a fresh raster, so
  a device upload of them belongs to the store entry, under its budget,
  rather than to the node.
- **Decided:** One sequential queue with one evaluator runs previews, export
  and auto exposure. Edits read no files, and project open reads only the
  project JSON. A preview or an action is a demand: a graph snapshot forced
  under one `GlobalContext` in one batch, whose effect then consumes the
  inputs. Actions use full detail. Demands are not merged. A later demand
  reuses what the evaluator's store kept from earlier ones (2026-10-13). A
  GUI action may return node parameters, applied as an ordinary edit.
- **Decided:** A worker serves one project. New and Open replace it: the old
  thread finishes its current command and skips its queue, and its replies
  have no receiver, so reused node IDs cannot pick up old results. New and
  Open are disabled while an action runs, since it cannot be interrupted and
  its result would be lost.

### Payloads and ports

- **Decided:** One `Rgb` payload. Interpretation is a typed descriptor value.
  Float identity is exact: only values copied from canonical constants are the
  same space, so an ICC profile near Rec.2020 is a different space.
- **Decided:** `Payload { NAME, Desc, Host, validate, check, allocate }`. The
  subtrait `DeviceLayout` (`byte_len`, `encode`, `decode`) marks payloads a
  kernel reads. Structured data stays on the CPU. Nodes flatten the numbers a
  kernel needs into per-invocation constants.
- **Decided (2026-10-13):** Ports name payloads, and residency is not part of
  a node signature. A node asks an input for host or device samples when it
  runs, and the missing representation is made once and counted. This
  replaces the `Cpu<P>`/`Device<P>` placements and the
  [TYPE_SYSTEM](TYPE_SYSTEM.md) principle that placement is known at connect
  time. The consumer decides where samples live, which the compute model
  needs before a planner or store can place materializations.
- **Decided:** Payloads: `Rgb` and `Bayer` (`HostBuffer` storage),
  `BayerLevels` (`[f32; 4]`) and host-only `CaptureMetadata`, whose `Desc` is
  the shared `Arc` of the decoded facts. Samples are linear. There is no scale
  or encoding descriptor until a producer differs.
- **Decided:** `RgbDesc { extent, full, coordinates }` with
  `Coordinates::{Camera(Response), Space(RgbSpace)}`. `Response { camera,
  balance }` holds make/model identity and the applied gains. Applying gains
  to balanced data composes the balance. `full` records the full-detail
  extent beside the stored one, so contracts derive preview geometry instead
  of reconstructing it from `extent · scale`, which is inexact after
  `div_ceil` reductions. `Display` impls supply every interpretation label.
- **Decided:** Requirements are port type arguments,
  `Read<'_, Rgb, Rec2020>`, defaulting to `Any`. On outputs they are
  guarantees checked on contract output. The checker, help and canvas labels
  read the same declaration.

### Nodes

- **Decided:** Contracts receive evaluated descriptions: `&Desc` for required
  inputs, `Option<&Desc>` for optional ones. An unconnected optional input is
  incomplete wiring: a node that needs it in some mode (CAT16 As shot) reports
  it at evaluation.
- **Decided:** Kernels, sources and maps are declared as functions with
  `#[node]` ([ARCHITECTURE](ARCHITECTURE.md#shapes)). Views and actions are
  kernels with an empty body. A map's contract describes its output like a
  kernel's, so both share one port and contract mechanism.
- **Decided (2026-10-13):** An output may be `Forward` instead of `Write`: the
  node publishes a raster it holds under the contract's new description,
  sharing storage. Published rasters are never written again, so sharing keeps
  nodes pure. Byte lengths must agree.
- **Decided:** Other files a node names (output ICC profiles in Export TIFF and
  Preview proofing) are assets: `#[node(assets)]` with `impl Assets for Params`.
- **Decided:** Headless actions are declared with the node,
  `#[node(action = export)]`. A kind has at most one action, a `GuiNode` extra
  or the node's. TIFF export is such a node in `drip`, and projects carry no
  separate export target. The CLI is a scaffold until batch work.
- **Decided:** RAW outputs `mosaic` and facts-only metadata. Consumers derive
  what they apply: Bayer gains from as-shot WB, Camera RGB conversion from
  `xyz_to_cam` and the image's balance, CAT16 the as-shot white. A user matrix
  becomes an optional port when chart calibration needs it.
- **Decided:** Each kind has one generated `static Kind`, and replacement
  compares kinds by identity. `#[node]` also generates typed `Inputs<'a>` and a
  `Typed` impl, which frontend extras bind through.
- **Decided:** Kinds are listed by hand, without link-time registration:
  `drip::registry::NODES` in pipeline order. A frontend composes a `Registry`
  from its lists and loads projects through it, and an unlisted kind cannot be
  added or loaded. Explicit lists are readable and avoid linker-section
  portability risks.

### Parameters and GUI bindings

- **Decided:** `#[param(default = 1.5, range = 0.5..=4.0)]`, bare `#[param]`
  for a choice's `#[default]` variant (`default = ...` overrides it),
  `open`/`save = [..]` and `flatten`. The typed default is the source of
  `Default` and the schema, and a const assertion checks field/schema
  agreement. Flattened fields validate with their parent. File roles live in
  the schema, and dialogs use its label and extensions.
- **Decided:** GUI extras are `impl GuiNode for <node>::Definition`, listed in
  `registry::BINDINGS` like kinds. Kinds without an entry use the schema
  controls. Test-only kinds and bindings live in `test_nodes`.

## Processing

- **Requirement:** Combine crop, rotation and perspective into Perspective
  Transform. Resample images by a factor, with interpolation and antialiasing.
- **Decided:** Implement JPEG export using `jpeg-encoder`, reusing output
  profiles and actions. Default to sRGB, quality 95 and 4:4:4. Quality and
  chroma subsampling remain independent controls. Reduced chroma uses area
  averaging. TIFF and JPEG share capture-tag writing and omit original
  orientation because stored pixels already carry their geometry.
- **Decided (2026-10-13):** Geometry is deferred and filtered once: geometry
  nodes are warps whose projective maps compose, and Resample chooses the
  filter and output size where it materializes. One footprint-scaled
  resampling stage serves interpolation and antialiasing ([FIELD](FIELD.md)).
  When geometry and per-pixel operations compose is under Compute. This
  replaces `RgbField` with Interpolate and Sample (2026-10-12).
- **Decided (2026-10-12):** Perspective Transform's geometry is one ordered
  four-point quad plus output aspect, in bijection with projective maps. The
  output is the quad's content, so crop and rotation are quads and have no
  parameters of their own. See [FIELD](FIELD.md#perspective-transform).
- **Decided (2026-10-12):** Points mapped outside the source are black, and
  edge taps repeat the edge. Auto-crop is a GUI constraint (Keep inside, on)
  plus a Fit action, not a stored mode. Resample defaults to Lanczos-3, and
  Perspective Transform resamples with it unless a Resample follows, accepting
  signed halos around HDR highlights. See
  [FIELD](FIELD.md#borders-and-auto-crop).
- **Decided (2026-10-12):** Perspective Transform has its own editing view,
  separate from Preview and without proofing, with Frame, Level and
  Perspective tools editing the same quad. A view's window may edit its node.
  See [FIELD](FIELD.md#editing-view).
- **Open:** Lens-correction coordinates (capture geometry, orientation and
  crop) and single-channel geometry for masks. See
  [FIELD](FIELD.md#open-questions). The compute model proposes masks as
  scalar images instead of a `MonoField` payload.
- **Tentative:** The node order in [TODO](TODO.md#next-image-nodes)
  prioritizes ordinary still-photo editing: input and colour/tone controls,
  then reusable local adjustments, followed by noise, sharpening and lens
  correction. Reuse existing exposure, CAT16 and sigmoid operations.
  Temperature/tint and neutral sampling extend the existing WB interface.
  This proposal does not change the packaged proof-of-concept gate.
- **Decided:** RAW metadata holds common capture/lens/GPS/sensor fields and
  native WB/color/black/white calibration. Derived values belong to
  consumers. Missing calibration does not hide descriptive metadata or mosaic
  data. Vendor maker notes are out of scope.
- **Decided:** Bayer site gains update samples and clipping levels in one node,
  and both outputs declare the same target coordinates. Capture facts stay
  immutable and do not describe the geometry of a transformed image.
- **Decided:** Keep as-shot gains through highlights and demosaic, then fold
  reference/as-shot gain ratios into the camera matrix, following darktable's
  “as shot to reference” placement (THIRD_PARTY [22]). Derive the scene white
  from the mapped camera neutral and apply CAT16 in linear Rec.2020 before
  rendering. The node accepts metadata or custom source xy. Target white is
  D65.
- **Decided:** Start with full CAT16 adaptation, composed into one working-RGB
  matrix. Full adaptation needs no scene luminance or surround estimate. Partial
  adaptation needs a policy: generalized/two-step von Kries keeps inverse and
  composition consistency [7], WGM models incomplete adaptation differently
  [8], and high-chroma experiments expose limits in all six tested CATs [9].
  Keep model choice separate from illuminant estimation. Temperature/tint is a
  white-point UI, not a CAT. Full CAM16/Hellwig modeling and mixed illuminants
  are separate [4], [5].
- **Decided:** EXIF orientation travels in capture metadata so contracts know
  the oriented extent before allocation. Decode LibRaw's convention at the RAW
  boundary, where unknown means identity. Orient once after camera conversion.
  Bayer processing keeps sensor coordinates. TIFF omits the original
  orientation tag.
- **Decided:** Exposure scaling is distinct from sensor-white normalization. The
  RAW denominator and sigmoid's 0.18 pivot do not establish ACES's 18% reflector
  scale [6]. Auto exposure is a one-shot GUI action that meters the linear
  Rec.2020 mean luminance of Exposure's input on the device and sets EV to
  `log2(0.18 / mean)`, which needs no current EV. Mean-to-18% is a starting
  point, not calibration.
- **Decided:** Signed/HDR coordinates are valid before rendering [3]. Sigmoid is
  a rendering operation although its output stays identity-encoded. LittleCMS
  with the selected intent handles output conversion. ACES target-aware
  rendering and source gamut compression are references, not prototype
  requirements [1], [2]. The Rec.2020/D65 working space stays for the prototype.
- **Decided:** Missing camera calibration is a supported-input limitation, not a
  reason to invent a matrix. Keep absent calibration distinct from extraction
  failures (Sigma DNGs have ColorMatrix tags but zero LibRaw `cam_xyz`).
- **Decided:** TIFF depth is u8, u16 (default) or f32. LittleCMS converts into
  the output encoding before quantization. Depth selects no transfer function.
- **Tentative:** Explain the scene-adjustment → rendering → encoding order in
  help and templates without an ordering graph check.
- **Requirement:** Sensor calibration needs matching sites, CFA phase and sample
  units. Separate decoding from black subtraction/normalization/crop or retain
  enough to account for them. Spatial gains may need spatial saturation/noise
  data.
- **Requirement:** Chart calibration produces a reusable transform with stated
  basis, normalization, reference illumination and fit residuals. Chart and
  subject may differ in grid and exposure.
- **Requirement:** Masks, gain fields and scalar samples have distinct contracts
  despite shared storage. Range, broadcasting, division and geometry rules belong
  to consumers. Applying a mask after filtering differs from restricting the
  filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate.
  Processing updates or discards metadata whose assumptions it changes.
- **Tentative:** Lens correction needs lens identification, crop factor and
  correction profiles. Vignetting may use focus distance, not minimum focus
  distance. Place correction before orientation or account for transformed
  coordinates. Distortion and lateral CA are per-channel coordinate maps.
  Vignetting is a per-pixel gain on `Rgb`.
- **Decided:** Multiple-frame operations consume decoded-value collections,
  with no graph-level frame-stack abstraction.

## Compute

- **Decided (2026-10-13):** Make the compiler embedded in evaluation explicit,
  as proposed in [COMPUTE_MODEL](COMPUTE_MODEL.md#proposal-for-drip): nodes
  declare a shape, evaluation fuses segments between materialization points,
  derives regions and preview scale from shapes, and memoizes in one store.
  It is executed in eight steps. Step 4 replaced the no-cache requirement
  under Performance with the store, and step 5 adds its disk tier.
- **Decided (2026-10-13):** Step 1. `Kind::shape` declares how evaluation runs
  a kind, `Kernel` or `Source`, and later shapes are added beside them. A
  run's values lived in a `Store`, which step 4 grew into the memo.
- **Decided (2026-10-13):** Step 2. A map node returns the constants of a
  `#[cube]` body over one pixel, and its contract still describes the output.
  Its output is a deferred image that the next map extends and the first
  consumer that needs samples materializes, once for all clones. Maps after a
  fan-out each recompute the shared plan in their own kernel instead of
  cutting the segment, which trades arithmetic for bandwidth and keeps
  results independent of evaluation order.
- **Decided (2026-10-13):** Step 3. A warp node (`#[node(warp)]`) returns
  per-channel projective maps from its output plane to its input plane in
  full-detail pixels, and the filter that reconstructs the input where the
  maps leave the sample lattice, or none for a reindex, which evaluation
  checks. Its contract describes ordinary `Rgb` on the output's lattice, so
  a domain origin folds into the maps and no field payload exists. A
  resample node (`#[node(resample)]`) returns its filter, and its contract
  sets the lattice from the size, the input's full extent and the global
  scale. Perspective Transform is a warp with Lanczos-3, Orientation a
  reindex warp, and Resample replaces Sample. Filters register as bodies, so
  the kernel names no node code.
- **Decided (2026-10-13):** A plan has one resampling stage before its map
  bodies, and a step composes into it only where the result is exact, so
  users never see the grouping. A reindex (Orientation, an integer crop)
  commutes with per-pixel maps and joins the stage wherever it sits. A
  filtering warp or Resample after a map cuts the segment: the maps
  materialize on their own lattice and the resampling runs as its own
  launch, because a weighted average of sigmoid values is not the sigmoid of
  the average. The compiler does not special-case linear maps. Before any
  map, warps and a Resample compose into one stage. A Resample fixes its
  lattice, so a filtering step after it resamples again. Preview scale
  reduces samples early, in demosaic. That is a preview-only approximation:
  export runs at full detail, and a Resample sets its lattice from
  full-detail extents instead of reducing. Step 6 moves the reduction to the
  first materialization finer than requested.
- **Decided (2026-10-13):** The stage loads samples when the composed map is
  the identity on an equal lattice and the filter interpolates, copies them
  when only reindexes composed, and filters otherwise. Its kind and filter
  are comptime, so a plan without geometry compiles to step 2's kernel. A
  plan that neither moves nor maps samples launches nothing and shares its
  input. An interpolating filter at factor one reproduces a reindex bit for
  bit, which Orientation's tests check with Lanczos-3 and Box.
- **Decided (2026-10-13):** The fused kernel is one `#[cube(launch)]` shell
  whose comptime argument is the body sequence, so each sequence compiles once
  and other constant values reuse it. Bodies keep the expression order of the
  kernels they replaced, so results are bit-identical. Fusion takes the colour
  tail from 6.6 ms to 1.8 ms at full detail on the Sony fixture
  ([PROGRESS](PROGRESS.md#2026-10-13)).
- **Decided (2026-10-13):** Step 4. The evaluator owns a `Store` that keeps
  materialized values across demands, keyed by content. An output's key is a
  SHA-256 over the producer's kind ID and `version`, its identity and the
  keys of its inputs in port order. A computed node's identity is its
  canonical parameter JSON (object keys sorted) and the global scale. A
  source's is its parameters and the stamps of the input files they name,
  without the scale. Bound in-memory data hashes a per-instance random
  identity. The kind ID determines a map's body, so input keys cover a
  plan's body sequence and constants without hashing them. `version` is
  bumped by hand when a kernel's numerics change. Equal nodes share one
  entry, and a new evaluator, as each GUI project has, starts empty.
- **Decided (2026-10-13):** Durability orders eviction: Low first, then
  Medium, then High, least recently used first within a class. A kind's
  durability is High for a source, otherwise the most durable of its tags,
  Low without tags. Tags state theirs: `io` High; `input-transform`,
  `bayer-raw`, `demosaic`, `recovery`, `geometry` and `color` Medium;
  `look`, `output-transform`, `tonemap`, `view` and `scope` Low. No node
  declares its own.
- **Decided (2026-10-13):** The store keeps rasters only, with each
  representation charged at the payload's `byte_len`, against a device
  budget (half the memory the runtime reports, 4 GiB where it reports none)
  and a host budget (8 GiB), both `RuntimeContext` settings. Over a budget it
  drops representations in eviction order. Dropping a device one keeps the
  host one, and an entry with neither leaves. Rasters sharing storage are
  charged once per entry. A deferred image enters the store once a consumer
  materializes it. One no consumer materialized is rebuilt by its nodes'
  host functions, so the store never holds a plan's input outside the
  budget.
- **Decided (2026-10-13):** Evaluation keys every needed node first, which
  loads sources, then walks back from the demanded values. A stored value
  is a hit, and a missing one schedules its node and the node's inputs.
  Target nodes of `evaluate_inputs` always run, so their contracts check
  what they receive. A run keeps its own live values with use counts, so a
  value the budget evicts mid-run stays available to its consumers.
  `Statistics` reports hits and evictions. A Sigmoid-only edit runs the five
  nodes of the tail segment and one launch, and an unchanged demand runs
  none.
- **Decided:** CubeCL computation with one kernel source per algorithm across
  runtimes. wgpu presentation and color management stay frontend-owned.
  `DRIP_BACKEND` selects explicitly and never falls back (crate README).
- **Requirement:** Do not distribute the CPU runtime: its LLVM JIT is too large.
  Windows uses D3D12. Distribution builds use the native candidates: Vulkan
  SPIR-V on Linux, native Metal on macOS, WGSL/D3D12 on Windows. xtask checks
  the target platform. These are performance candidates, not measured winners.
- **Decided:** RCD scratch is allocated before its first pass and released after
  its last dispatch. CubeCL owns queued bindings. No host waits.
- **Decided:** Reductions use fixed-order partial sums without float atomics.
  Exposure reductions cap lanes at the runtime limit, since CPU barrier kernels
  need a host worker per lane. Scopes compare uploaded f32 thresholds to keep
  exact bin edges. The vectorscope normalizes RGB before XYZ for HDR headroom.
- **Decided:** GUI scopes and auto exposure consume device RGB with GUI-owned
  kernels and read back counts or one f32 until step 7 adds `reduce`.
  Softproof, gamut check and preview packing stay on the CPU. Packing uses
  Rayon over disjoint RGBA f16 slices.

## Performance

- **Requirement:** Apply mechanical simplifications that reduce complexity first.
  Ownership, scheduling and contract changes remain separate work.
- **Decided:** GUI bindings stay frontend-owned. The editor uses core port
  handles and saves names. Canvas layout and drawing share a node snapshot per
  pass. The worker keeps packed preview images until they are released, not
  their source pixels.
- **Tentative:** Presentation-only edits need current drawing settings or
  re-preparation of retained results. Suppressing evaluation alone leaves
  prepared settings stale. Keep typed single-output evaluation as public API
  even while only tests call it.
- **Tentative:** Preview levels (powers of two) stay in the frontend. The core
  accepts any integer scale and validates it at evaluation, since
  `GlobalContext::scale` is public.
- **Decided:** Render tests use production packing, upload, binding and pipeline
  setup. Expected color values remain independent of that implementation.
- **Decided:** The GPU display test initializes the selected compute runtime
  before its independent wgpu instance. CubeCL serializes compute startup,
  which prevents concurrent first Vulkan calls that crash the NVIDIA ICD while
  allowing parallel tests. The display test therefore also requires the
  selected `DRIP_BACKEND` to be available.
- **Requirement:** Do not optimize prematurely. Tracing adds no
  synchronization. Benchmark artifacts stay outside Git.
- **Decided (2026-10-13):** Materialized values persist across demands in
  the evaluator's store under byte budgets ([Compute](#compute), step 4).
  This replaces the requirement of no persistent intermediate cache. The
  user approved the compute model's plan that replaces it.
- **Decided:** Sub-300 ms latency is acceptable without further optimization.
- **Decided:** Benchmark protocol: Sony fixture (7968×5320), release builds,
  WGPU on the RTX 3080, `RAYON_NUM_THREADS=12`, serial runs. Drive the real GUI
  worker with Preview, Histogram, Waveform, Vectorscope and Export targets.
  Warm up levels 2, 1, 0, repeat them five times, alternate ±0.1 EV five times
  at level 1, and issue five identical level-1 requests. Report medians and
  ranges. Exclude drawing, texture upload and file export. Harness and results
  live in `../drip-benchmark-results/`.
- **Tentative (2026-10-13):** The benchmark protocol's ±0.1 EV alternation
  returns to stored values from its third sample. The step 4 measurement
  adds five level-1 requests with a new EV each to time a recomputed tail.

## GUI

- **Requirement:** Preserve universal node controls/views, per-node pop-out
  windows and interaction logic. Internal APIs may change.
- **Decided:** Sentence-case display labels independent of persisted keys. IDs
  name the operation alone in kebab case, qualified where it would be unclear
  (`rcd-demosaic`, `export-tiff`), without migration aliases.
- **Decided:** Nodes carry tags from one list, `drip::registry::TAGS` (`id`,
  `description`), named by path so a misspelling fails to compile. Frontends
  declare none of their own, so `view` and `scope` live there too. Tags say
  what a node is for, since ports and payloads already say what it accepts. At
  most one stage per node, by convention rather than a check: the ACES input
  transform, look or output transform. Exposure is a look. Geometry, views and
  file writers have no stage. RAW decoding and chromatic adaptation (as-shot
  white by default) count as input transforms. `io` marks nodes that read or
  write files, crossing stages. `bayer-raw` marks work on the undemosaiced
  Bayer mosaic, so X-Trans or monochrome sensor data can later take tags of
  their own.
- **Decided:** Nodes are added through a palette, an `egui::Modal` sized to
  the window, opened by a right-click on the background or Space over the
  canvas (the node goes under the pointer) or "Add node…" in the menu (the
  centre of the visible canvas). Search matches name, ID and tags, ranking
  name prefixes, then word prefixes, then other matches. Tag chips narrow to
  kinds carrying every chosen tag. Each result shows its tags and its help's
  first sentence. Filtering by a dragged wire's payload would read ports, not
  tags.
- **Decided:** Enum variants own their dependent parameter structs. Serde
  adapters and UI schemas are generated together, externally tagged. Only the
  selected variant's fields are shown and saved.
- **Decided:** Projects use `.drip`, templates `.drip-template`. Opening a
  template creates an untitled project. Exposed template parameters reset to
  their defaults when saving a template.
- **Decided:** Project commands share one table for menu buttons and
  shortcuts, which match modifiers exactly in every window: New Ctrl+N, Open
  Ctrl+O, Save Ctrl+S, Save as Ctrl+Shift+S, Save template Ctrl+Alt+S, Undo
  Ctrl+Z, Redo Ctrl+Shift+Z (Cmd on macOS). Button tooltips show them. Focused
  text fields keep their keys.
- **Decided:** Undo restores whole-graph snapshots: DAG, names, template
  inputs and layout, not the preview level, selection or windows. Nodes are
  shared `Arc`s, so a step copies entries and labels. A replaced node and its
  loaded file stay in memory while a step refers to it, which is intended. A
  step ends when the pointer is released in the window that edited. Text
  fields edit once on losing focus, and action results are steps of their own.
  `DRIP_HISTORY` sets the depth (default 100).
- **Decided:** New and Open ask to save, discard or cancel unsaved edits in
  every window. A failed or canceled save keeps the project.
- **Decided:** Ports show declared payload names and endpoint names, and
  optional inputs are marked `?`. Long titles are elided with full names on
  hover. Unsaved edits are tracked separately from evaluation requests.
- **Decided:** Scope overlays keep axes and compact color-space labels, with no
  counts, dimensions or repeated settings. Waveforms default to logarithmic
  density, histograms to linear.
- **Decided:** Text is set in Source Sans 3 at weight 400, names (palette
  results, the inspector's name field) at 600, and monospace in Source Code
  Pro, both vendored variable fonts embedded in the binary. egui's bundled
  fonts are disabled, so glyphs outside these fonts render as `?`: no emoji,
  and node header buttons use `≡` (parameters) and `❒` (view). Every test
  harness installs the fonts, so text is measured as shipped.
- **Decided:** Inspector help joins Rustdoc source wraps and keeps paragraphs.
  Reflow belongs to presentation.
- **Decided (2026-10-12):** Built-in templates are ordinary `.drip-template`
  files in the repository's `templates/`, embedded in the GUI with
  `include_str!` and listed by hand in `templates.rs`. They load through the
  same path as a template a user opens. New creates the first (Neutral
  restore). A test requires each listed file to equal its own
  re-serialization, so a schema or format change regenerates them. They carry
  the crate's major version and are not migrated. They contain GUI view kinds,
  so a headless frontend cannot load them as they are. The core keeps only a
  test fixture of the RAW chain.
- **Decided:** Neutral restore uses a landscape layout: views share a top edge,
  processing staggers down through Orientation, and RAW and TIFF sit at the
  bottom corners.
- **Tentative:** Rounded orthogonal wires could help backward links. Routing
  around nodes needs obstacle-aware paths. No router chosen.

## Distribution

- **Decided:** Headless CI builds `drip/cpu` and tests with `DRIP_BACKEND=cpu`.
  GPU tests are opt-in. Tests on every platform gate distribution, and
  distribution jobs only build and upload. Clippy uses `--workspace
  --no-deps`. Validate test changes with `drip/cpu` enabled too, since
  default-feature builds omit CPU-only integration tests.
- **Decided:** Worker tests allow 90 seconds for compute completion, including
  a dropped worker finishing its current preview. CPU tests share a compute
  client with full-RAW evaluations. Synchronization gates retain five seconds.
- **Requirement:** Minimize user-installed dependencies. Native processing
  libraries are static. The normal Linux C/C++ runtime may stay external.
- **Decided:** Vendor unmodified sources at release tags. Builds never fetch.
  Pins and licenses are in [THIRD_PARTY](../THIRD_PARTY.md). LibRaw uses its
  shared source manifest through `cc` on Windows too.
- **Decided:** Preserve user compiler flags and targets. Defer CPU variants and
  OpenMP until measured gains justify their packaging cost.
- **Decided:** Build each target natively on the newest standard GitHub runners.
- **Tentative:** Linux `.tar.xz`/Flatpak, unsigned macOS `.app` ZIPs, Windows ZIPs.

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
