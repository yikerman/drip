# Design

Decisions not evident from code. Code principles are in `AGENTS.md`; node,
port and contract principles and their mathematics in
[TYPE_SYSTEM](TYPE_SYSTEM.md). Unfinished work is in [TODO](TODO.md),
validation in [PROGRESS](PROGRESS.md).

Status: **Requirement** is stated by the user; **Decided** is agreed;
**Tentative** and **Open** are unsettled and may be challenged.

## Scope

- **Requirement:** The README addresses users: highlight CubeCL computation,
  freely arranged controls/views, color-managed output and pipeline checks.
  Explain only the unusual canvas gestures; leave conventional node controls
  to the interface. Demo captures use the Sony ILCE-7RM3 fixture and show only
  Drip. Use cheerful, benefit-led product copy and show every supplied demo
  inline with its own visible caption. Identify gamut warnings in the caption.
- **Decided:** The next milestone is a packaged, user-testable proof of concept,
  including scene-illuminant white balance and orientation (both implemented).
  Other image-quality and editing limitations are accepted; broader reference
  validation and SpyderX measurements are not blockers. Verify installation,
  launch and a complete RAW-to-TIFF smoke test.
- **Decided:** “Scientific” for the prototype means sound, documented image
  processing, not calibrated radiometry or ACES conformance. Quantitative
  sensor/chart calibration is outside this milestone.
- **Requirement:** Correct display color on Wayland, macOS and Windows. Linux is
  the primary development platform; X11 is unsupported.
- **Decided:** Prototype project formats have no migration guarantee. The
  format version is the `drip` crate's major version; other versions are
  rejected.

## Model

Settled with the user on 2026-10-11 after a review against
[TYPE_SYSTEM](TYPE_SYSTEM.md) and `AGENTS.md`, and implemented.

### Evaluation

- **Decided:** Evaluation is lazy, after Haskell's model. Each check depends
  only on what its event supplies. A graph edit checks ports: payload types,
  occupancy and cycles. Admission checks parameters only (`#[validate(fn)]` on
  the parameter struct; schema ranges guide controls). Contracts, requirements,
  missing inputs and file reads happen when evaluation forces the node and are
  reported on it; downstream nodes name it. A branch nothing forces reports
  nothing. Edit-time contracts made the same edit pass or fail depending on
  whether a RAW was loaded, so they were removed.
- **Decided:** Only views and actions force evaluation, as `main` forces
  thunks. A GUI preview targets the nodes with a view; Export runs when its
  action runs.
- **Decided:** A node reads its files (source outputs or assets) when first
  forced and keeps the result, failure included, in a generated `OnceLock`.
  Files are assumed not to change, so the cache does not change the node's
  outputs; it holds file contents, not intermediate results. Graph snapshots
  share it; a parameter edit admits a new node, which reads again. To pick up a
  changed file, choose it again.
- **Decided:** One sequential queue with one evaluator runs previews, export
  and auto exposure. Edits read no files; project open reads only the project
  JSON. A preview or an action is a `Demand`: a graph snapshot forced under one
  `GlobalContext` in one batch (shared ancestors run once; each output lives
  until its last consumer finishes), then its effect consumes the inputs.
  Actions use full detail. Demands are not merged: reuse would need the
  excluded cache. A GUI action may return node parameters, applied as an
  ordinary edit.
- **Decided:** A worker serves one project. New and Open replace it: the old
  thread finishes its current command, skips its queue, and its replies have no
  receiver, so reused node IDs cannot pick up old results. New and Open are
  disabled while an action runs, since it cannot be interrupted and its result
  would be lost; actions are disabled only while another action runs.

### Payloads and ports

- **Decided:** One `Rgb` payload; interpretation is a typed descriptor value.
  Float identity is exact: only values copied from canonical constants are the
  same space, so an ICC profile near Rec.709 is a different space.
- **Decided:** `Payload { NAME, Desc, Host, validate, check, allocate }`. The
  subtrait `DeviceLayout` (`byte_len`, `encode`, `decode`) marks payloads a
  kernel reads; `Device<P: DeviceLayout>` and `Cpu<P>` are port placements, so
  a device port of a host-only payload does not compile. Transfers use the
  device-side port's functions. Structured data stays on the CPU; nodes flatten
  the numbers a kernel needs into per-invocation constants.
- **Decided:** Payloads: `Rgb` and `Bayer` (`HostBuffer` storage),
  `BayerLevels` (`[f32; 4]`) and host-only `CaptureMetadata`, whose `Desc` is
  the shared `Arc` of the decoded facts. Samples are linear; there is no scale
  or encoding descriptor until a producer differs.
- **Decided:** `RgbDesc { extent, coordinates }` with
  `Coordinates::{Camera(Response), Space(RgbSpace)}`; `Response { camera,
  balance }` holds make/model identity and the applied gains;
  `BayerDesc { extent, phase, response }`. Applying gains to balanced data
  composes the balance. `Display` impls supply every interpretation label.
- **Decided:** Requirements are port type arguments,
  `Read<'_, Device<Rgb>, Rec2020>`, defaulting to `Any`; on `Write` they are
  guarantees checked on contract output. The checker, help and canvas labels
  read the same declaration.

### Nodes

- **Decided:** Contracts receive evaluated descriptions: `&Desc` for required
  inputs, `Option<&Desc>` for optional ones. Requirement checks run first in
  the same generated path. An unconnected optional input is incomplete wiring:
  a node that needs it in some mode (CAT16 As shot) reports it at evaluation.
- **Decided:** Two declaration forms. Computation, view and action nodes:
  `fn(&KernelContext, &Params, Read..., Write...) -> Result<()>`, with an empty
  body for views and actions. Sources: `fn(&Params, Load<'_, P>...)`, run once
  when first forced; `Load::set` validates at the boundary.
- **Decided:** Other files a node names (output ICC profiles in Export TIFF and
  Preview proofing) are assets: `#[node(assets)]` with `impl Assets for Params`.
  Actions and preparations receive the admitted node and its assets.
- **Decided:** Headless actions are declared with the node,
  `#[node(action = export)]`, as `fn(&Params, [&Loaded,] Inputs<'_>)`; the
  label comes from the function name. A kind has at most one action, a
  `GuiNode` extra or the node's. TIFF export is such a node in `drip`; projects
  carry no separate export target. The CLI is a scaffold until batch work.
- **Decided:** RAW outputs `mosaic` and facts-only metadata. Consumers derive
  what they apply: Bayer gains from as-shot WB, Camera RGB conversion from
  `xyz_to_cam` and the image's balance, CAT16 the as-shot white. A user matrix
  becomes an optional port when chart calibration needs it.
- **Decided:** Each kind has one generated `static Kind` (ID, labels, category,
  help, references, schema, defaults, ports, action); replacement compares kinds
  by identity. `#[node]` also generates typed `Inputs<'a>` and a `Typed` impl;
  the kernel adapter and frontend extras bind through it, and
  `InputValues::bind::<N>()` checks the kind. Host and device buffers clone as
  shared immutable storage.
- **Decided:** Kinds are listed by hand, without link-time registration:
  `drip::node::NODES` in pipeline order. A frontend composes a `Registry`
  from its lists (the GUI adds its views) and loads projects through it; an
  unlisted kind cannot be added or loaded. Explicit lists are readable and
  avoid linker-section portability risks.

### Parameters and GUI bindings

- **Decided:** `#[param(default = 1.5, range = 0.5..=4.0)]`, bare `#[param]`
  for a choice's `#[default]` variant (`default = ...` overrides it),
  `open`/`save = [..]` and `flatten`. The typed default is the source of
  `Default` and the schema; a const assertion checks field/schema agreement.
  Flattened fields validate with their parent. File roles live in the schema;
  dialogs use its label and extensions.
- **Decided:** `impl GuiNode for <node>::Definition`, listed in
  `node_ui::BINDINGS` like kinds: optional `PREPARE` and `ACTION` receive the
  admitted node and typed `Inputs`; controls default to the schema. Kinds
  without an entry use the defaults. `PrepareContext` carries the demand's
  `GlobalContext`. Test-only kinds and bindings live in `test_nodes`.

## Processing

- **Decided:** RAW metadata holds common capture/lens/GPS/sensor fields and native
  WB/color/black/white calibration (derived values move to consumers; see Model). Missing calibration does not hide descriptive metadata or mosaic
  data. Vendor maker notes are out of scope.
- **Decided:** Bayer site gains update samples and clipping levels in one node;
  both outputs declare the same target coordinates. Capture facts stay immutable
  and do not describe the geometry of a transformed image.
- **Decided:** Keep as-shot gains through highlights and demosaic, then fold
  reference/as-shot gain ratios into the camera matrix, following darktable's
  “as shot to reference” placement (THIRD_PARTY [22]). Derive the scene white
  from the mapped camera neutral and apply CAT16 in linear Rec.2020 before
  rendering. The node accepts metadata or custom source xy; target white is D65.
- **Decided:** Start with full CAT16 adaptation, composed into one working-RGB
  matrix. Full adaptation needs no scene luminance or surround estimate. Partial
  adaptation needs a policy: generalized/two-step von Kries keeps inverse and
  composition consistency [7]; WGM models incomplete adaptation differently [8];
  high-chroma experiments expose limits in all six tested CATs [9]. Keep model
  choice separate from illuminant estimation; temperature/tint is a white-point
  UI, not a CAT. Full CAM16/Hellwig modeling and mixed illuminants are separate [4], [5].
- **Decided:** EXIF orientation travels in capture metadata so contracts know
  the oriented extent before allocation. Decode LibRaw's convention at the RAW
  boundary; unknown means identity. Orient once after camera conversion; Bayer
  processing keeps sensor coordinates. TIFF omits the original orientation tag.
- **Decided:** Exposure scaling is distinct from sensor-white normalization; the
  RAW denominator and sigmoid's 0.18 pivot do not establish ACES's 18% reflector
  scale [6]. Auto exposure is a one-shot GUI action that meters the linear
  Rec.2020 mean luminance of Exposure's input on the device and sets EV to
  `log2(0.18 / mean)`, which needs no current EV. Mean-to-18% is a starting
  point, not calibration.
- **Decided:** Signed/HDR coordinates are valid before rendering [3]. Sigmoid is
  a rendering operation although its output stays identity-encoded; LittleCMS
  with the selected intent handles output conversion. ACES target-aware
  rendering and source gamut compression are references, not prototype
  requirements [1], [2]. The Rec.2020/D65 working space stays for the prototype.
- **Decided:** Missing camera calibration is a supported-input limitation, not a
  reason to invent a matrix. Keep absent calibration distinct from extraction
  failures (Sigma DNGs have ColorMatrix tags but zero LibRaw `cam_xyz`).
- **Decided:** TIFF depth is u8, u16 (default) or f32. LittleCMS converts into
  the output encoding before quantization; depth selects no transfer function.
- **Tentative:** Explain the scene-adjustment → rendering → encoding order in
  help and templates without an ordering graph check.
- **Requirement:** Sensor calibration needs matching sites, CFA phase and sample
  units; separate decoding from black subtraction/normalization/crop or retain
  enough to account for them. Spatial gains may need spatial saturation/noise data.
- **Requirement:** Chart calibration produces a reusable transform with stated
  basis, normalization, reference illumination and fit residuals. Chart and
  subject may differ in grid and exposure.
- **Requirement:** Masks, gain fields and scalar samples have distinct contracts
  despite shared storage. Range, broadcasting, division and geometry rules belong
  to consumers. Applying a mask after filtering differs from restricting the
  filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate;
  processing updates or discards metadata whose assumptions it changes.
- **Tentative:** Lens correction needs lens identification, crop factor and
  correction profiles; vignetting may use focus distance, not minimum focus
  distance. Place correction before orientation or account for transformed
  coordinates.
- **Decided:** Multiple-frame operations consume decoded-value collections;
  no graph-level frame-stack abstraction.

## Compute

- **Decided:** CubeCL computation with one kernel source per algorithm across
  runtimes; wgpu presentation and color management stay frontend-owned.
  `DRIP_BACKEND` selects explicitly and never falls back (crate README).
- **Requirement:** Do not distribute the CPU runtime; its LLVM JIT is too large.
  Windows uses D3D12. Distribution builds use the native candidates: Vulkan
  SPIR-V on Linux, native Metal on macOS, WGSL/D3D12 on Windows; xtask checks
  the target platform. These are performance candidates, not measured winners.
- **Decided:** RCD scratch is allocated before its first pass and released after
  its last dispatch; CubeCL owns queued bindings. No host waits.
- **Decided:** Reductions use fixed-order partial sums without float atomics.
  Exposure reductions cap lanes at the runtime limit; CPU barrier kernels need a
  host worker per lane. Scopes compare uploaded f32 thresholds to keep exact bin
  edges; vectorscope normalizes RGB before XYZ for HDR headroom.
- **Decided:** GUI scopes and auto exposure consume device RGB with GUI-owned
  kernels and read back counts or one f32. Softproof, gamut check and preview
  packing stay on the CPU; packing uses Rayon over disjoint RGBA f16 slices.

## Performance

- **Requirement:** Apply mechanical simplifications that reduce complexity first;
  ownership, scheduling and contract changes remain separate work.
- **Decided:** GUI bindings stay frontend-owned. The editor uses core port
  handles and saves names. Canvas layout and drawing share a node snapshot per
  pass. The worker keeps packed preview images until they are released, not
  their source pixels.
- **Tentative:** Presentation-only edits need current drawing settings or
  re-preparation of retained results; suppressing evaluation alone leaves
  prepared settings stale. Keep typed single-output evaluation as public API
  even while only tests call it.
- **Tentative:** Preview levels (powers of two) stay in the frontend; the core
  accepts any integer scale and validates it at evaluation, since
  `GlobalContext::scale` is public.
- **Decided:** Render tests use production packing, upload, binding and pipeline
  setup; expected color values remain independent of that implementation.
- **Decided:** The GPU display test initializes the selected compute runtime
  before its independent wgpu instance. CubeCL serializes compute startup;
  this prevents concurrent first Vulkan calls that crash the NVIDIA ICD while
  allowing parallel tests. The display test therefore also requires the
  selected `DRIP_BACKEND` to be available.
- **Open:** A RAW node keeps its decoded host values; keeping the device
  representation too would avoid a per-demand upload. Define runtime ownership,
  lifetime and memory cost first. Upload volume alone does not establish the
  largest remaining latency cost.
- **Requirement:** No persistent intermediate cache; do not optimize prematurely.
  Tracing adds no synchronization. Benchmark artifacts stay outside Git.
- **Decided:** Sub-300 ms latency is acceptable without further optimization.
- **Decided:** Benchmark protocol: Sony fixture (7968×5320), release builds,
  WGPU on the RTX 3080, `RAYON_NUM_THREADS=12`, serial runs. Drive the real GUI
  worker with Preview, Histogram, Waveform, Vectorscope and Export targets.
  Warm up levels 2, 1, 0; repeat them five times; alternate ±0.1 EV five times
  at level 1; issue five identical level-1 requests. Report medians and ranges;
  exclude drawing, texture upload and file export. Harness and results live in
  `../drip-benchmark-results/`.
- **Open:** Bounded reuse for exposure/sigmoid edits conflicts with the no-cache
  requirement; it would need a byte budget and context invalidation.

## GUI

- **Requirement:** Preserve universal node controls/views, per-node pop-out
  windows and interaction logic; internal APIs may change.
- **Decided:** Sentence-case display labels independent of persisted keys. IDs
  use `category.operation` without migration aliases. Categories: Input, Sensor,
  Demosaic, Color, Tone, Geometry, View, Output.
- **Decided:** Enum variants own their dependent parameter structs; Serde
  adapters and UI schemas are generated together, externally tagged. Only the
  selected variant's fields are shown and saved.
- **Decided:** Projects use `.drip`, templates `.drip-template`. Opening a
  template creates an untitled project; exposed template parameters reset to
  their defaults when saving a template.
- **Decided:** Project commands share one table for menu buttons and
  shortcuts, which match modifiers exactly in every window: New Ctrl+N, Open
  Ctrl+O, Save Ctrl+S, Save as Ctrl+Shift+S, Save template Ctrl+Alt+S, Undo
  Ctrl+Z, Redo Ctrl+Shift+Z (Cmd on macOS). Button tooltips show them; New and
  Open wait for a running action. Focused text fields keep their keys.
- **Decided:** Undo restores whole-graph snapshots: DAG, names, template
  inputs and layout, not the preview level, selection or windows. Nodes are
  shared `Arc`s, so a step copies entries and labels; a replaced node and its
  loaded file stay in memory while a step refers to it, which is intended. A
  step ends when the pointer is released in the window that edited; text
  fields edit once on losing focus, and action results are steps of their own.
  `DRIP_HISTORY` sets the depth (default 100). Undo marks the project modified.
- **Decided:** New and Open ask to save, discard or cancel unsaved edits in
  every window; a failed or canceled save keeps the project.
- **Decided:** Ports show declared payload names and endpoint names; optional
  inputs are marked `?`. Long titles are elided with full names on hover.
  Unsaved edits are tracked separately from evaluation requests.
- **Decided:** Scope overlays keep axes and compact color-space labels; no
  counts, dimensions or repeated settings. Waveforms default to logarithmic
  density, histograms to linear.
- **Decided:** Inspector help joins Rustdoc source wraps and keeps paragraphs;
  reflow belongs to presentation.
- **Decided:** The default RAW template uses a landscape layout: views share a
  top edge, processing staggers down through Orientation, RAW and TIFF sit at
  the bottom corners.
- **Tentative:** Rounded orthogonal wires could help backward links; routing
  around nodes needs obstacle-aware paths. No router chosen.

## Distribution

- **Decided:** Headless CI builds `drip/cpu` and tests with `DRIP_BACKEND=cpu`;
  GPU tests are opt-in. Tests on every platform gate distribution; distribution
  jobs only build and upload. Clippy uses `--workspace --no-deps`.
  Validate test changes with `drip/cpu` enabled too: default-feature builds
  omit CPU-only integration tests.
- **Decided:** Worker tests allow 90 seconds for compute completion, including
  a dropped worker finishing its current preview. CPU tests share a compute
  client with full-RAW evaluations; synchronization gates retain five seconds.
- **Requirement:** Minimize user-installed dependencies; native processing
  libraries are static, the normal Linux C/C++ runtime may stay external.
- **Decided:** Vendor unmodified sources at release tags; builds never fetch.
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
