# Design intent

Current decisions and rationale. Detailed contracts live beside the code;
[PROGRESS](PROGRESS.md) records validation, [TODO](../TODO.md) unfinished work,
and [THIRD_PARTY](../THIRD_PARTY.md) algorithm and dependency credits.
Statuses: **requirement** = user goal; **decided** = agreed direction;
**tentative** = proposal; **open** = unresolved.

## Direction and boundaries

- **Decided:** v0.1 priorities are production-pipeline validation, editing UI/UX,
  SpyderX color verification, then packaging. CLI expansion and speculative
  optimization follow those priorities.
- **Requirement:** The library owns processing, graph validation, color management
  and persistence, with no GUI code or GUI data preparation. Support correct color
  on Linux/Wayland, macOS and Windows; Linux is the primary development platform.
  X11 is unsupported. A Linux-only color path does not qualify a replacement GUI.
- **Requirement:** Use LibRaw for decoding and LittleCMS for ICC transforms.
  The minimal owned-data [LibRaw binding](../crates/drip-libraw/src/lib.rs) isolates
  version-dependent native layouts behind a C shim.
- **Requirement:** Group code by responsibility. Keep one computational kernel
  source across backends; portability and ease of development outrank peak speed.

## DAG contracts

### Philosophy and goals

- **Requirement:** Permit compositions whose declared local contracts agree,
  including unusual creative orderings. Recommend useful pipelines without
  enforcing photographic taste or pretending to prove scene fidelity.
- **Decided:** Ordinary color processing uses concrete linear Rec.2020/D65 RGB.
  A creative curve establishes new values under that interpretation; it need not
  preserve captured-light ratios. Exposure or blur after sigmoid is meaningful.
  Gamma-encoded RGB, camera-native coordinates and sensor mosaics are different
  representations and need explicit conversion before ordinary RGB processing.
- **Requirement:** Kernels compute from parameters, input values and explicit
  context. Computation is pure; caches do not change results. External effects
  such as file export are actions. Graphs are acyclic.
- **Requirement:** Reject representation mismatches and unmet algorithmic
  prerequisites. Keep recommendations about calibration quality, module order
  and intended looks in node help. Numerical preconditions belong to the node
  that needs them; there is no universal positive/bounded-pixel rule.

### Case studies and future requirements

- **Sensor calibration:** dark subtraction takes two mosaics, not two file paths.
  Flat preparation may produce a reusable gain field. Corresponding sensor sites,
  CFA phase and sample units matter; matching filenames or dimensions alone do
  not establish them. RAW decoding must eventually expose unmodified samples or
  retain enough information to account for black subtraction, normalization and
  crop. Current `raw.read` still performs those steps together.
- **Chart calibration:** sample patches from a separate photograph, fit a transform
  against reference values, then apply that transform to another image. Chart
  and subject need not share dimensions or exposure. Transform inputs must use
  its declared basis/normalization. Reference illumination and fit residuals are
  explicit data, not inferred guarantees from a profile's filename.
- **Masks and pixel math:** share arithmetic kernels across concrete wrappers.
  Scalar fields, masks and gain fields may share storage without being the same
  contract. Define mask range/broadcasting and division behavior locally. A
  typed sample-extraction/reinterpretation operation can support experiments
  without silently treating sensor coordinates as working RGB.
- **Filtering and geometry:** blending a blurred image through a mask differs
  from restricting a filter's samples using that mask. Geometry and boundary
  policies belong to each operation. Index-wise blending may deliberately pair
  unrelated images; same-sensor calibration requires sensor-site correspondence.
- **Noise and metadata:** capture EXIF is provenance, not a noise-model promise.
  Transform or discard applicability-dependent metadata when processing changes
  it. Spatial gains can require spatial saturation/noise data; copying old
  channel thresholds is not sufficient. Do not add a history-wide proof system.
- **Multiple frames:** decoded-value collections can feed denoise, HDR, super-
  resolution or stitching nodes. Reusable transforms/registrations can be separate
  outputs, but combined nodes may keep alignment internal. Typed collection
  payloads require no graph-level frame-stack abstraction.

### Declarations and execution

- **Decided:** `RawMat<C>` is storage; `RealMat<C, I>` adds concrete interpretation
  metadata. `Mosaic`, `CameraRgb` and `Rec2020Mat` are distinct Rust types.
  `Interpretation` supplies only a name. Other payloads implement `EdgeValue`
  locally; they need no central enum or manual catalogue entry.
- **Decided:** `#[node(...)]` accepts a function body or a declaration ending in
  `;`. Its signature supplies parameter/input/output types. Rustdoc supplies help;
  attributes supply stable ID, category, name, port names, checks/actions and
  references. The macro generates the typed handle, adapters and linked discovery.
- **Decided:** Inputs are `&T`, `Either<&A, &B>`, or `Option` of either. Alternatives
  are finite concrete choices, used by histogram/waveform for working or camera
  RGB. They confer no inherited capabilities. Wrapper names must appear in the
  signature (qualified paths work); proc macros do not resolve wrapper aliases.
- **Decided:** Outputs are tuples of `Arc<T>`. Every output has a known concrete
  type before evaluation. `Value` constructs its payload and `TypeId` descriptor
  together. Sharing a display name never establishes compatibility. No reflection
  dictionaries, interpretation-witness checks or input-dependent output inference.
- **Decided:** Connection edits check endpoints, cycles and accepted concrete
  types before replacing an edge. Fixed outputs make downstream type propagation
  unnecessary. Unconnected inputs affect evaluability, not output type knowledge.
- **Decided:** Evaluation uses the same input contracts, then runs node-local
  typed checks on actual values before invoking a kernel, action or observer.
  Optional inputs permit absence, not errors or incompatible connected data.
  Diagnostics identify the affected node/port or named relationship.
- **Decided:** Rust checks signatures; implementers remain responsible for sample
  meaning and numerical correctness. Contracts do not certify arbitrary kernels,
  chart quality, sensor calibration or a photograph's relationship to its scene.
- **Decided:** `Parameters` and `Choice` derive schema, validated typed access,
  defaults, persistence names and documentation from local declarations. GUI
  labels and errors use declared payload names verbatim.
- **Decided:** Evaluation caches values and failures by dependencies, parameters
  and resolution. RAW/ICC reads share explicit resource snapshots; manual
  invalidation refreshes same-path changes. The concrete-port rewrite leaves
  resource lifetime, actions and GUI preparation boundaries unchanged.

The executable [calibration DAG](../crates/drip/examples/calibration_dag.rs)
demonstrates concrete sensor values, a reusable calibration branch, type rejection,
value-dependent grid checks and cache recovery. It uses synthetic common-unit
samples; it is not a production dark/flat or chart-profiling implementation.
The [masked-edit DAG](../crates/drip/examples/masked_edit_dag.rs) demonstrates
ordinary RGB processing, a scalar-mask payload and blending a branch containing
sigmoid followed by exposure. Both examples run as tests in CI.

## Computational and GUI nodes

- **Decided:** GUI nodes own preparation, presentation values, drawing and local
  interaction. Preview proofing/gamut overlays and scope visualization live in
  `drip-gui`; export and reusable ICC conversion remain in `drip`.
- **Decided:** `GuiNode` binds the exact typed declaration to its preparation
  signature and result. Private erased adapters enforce this boundary.
  `#[gui_node]` generates discovery beside the implementation. `Presentation` is
  the node's prepared value; `IntoDrawable` converts it to `Drawable` on the worker,
  using `ImageCache` for shared pixel packing and retirement. Image drawables live
  beside their implementations. There is no central presentation enum.
- **Decided:** A preparation callback enables the standard resizable viewer and
  its single `Part::View` pop-out, even before preparation succeeds. Custom controls
  retain that viewer; ordinary nodes use schema controls. There is no separate
  outer view override or named collection of views without a concrete consumer.
- **Decided:** Controls use parameters without requiring evaluated inputs.
  Worker preparation uses `Evaluator::with_inputs`, evaluating dependencies but
  not the observed kernel. It reuses checks from matching successful evaluation
  or validates inputs once. Add output observation only for a concrete consumer.
- **Decided:** Presentation caches are separate from computational results/errors.
  Track parameters, dependencies, detail and resource snapshots; cache preparation
  failures too. Share views across canvas/pop-outs, retain their generation while
  showing previous results, and reject stale work. Resource invalidation clears
  results while preserving worker ownership of image allocations until retirement.
- **Decided:** GUI modules follow computational responsibilities, not a mirrored
  file tree. Complex nodes may split preparation, proofing, drawing and tests.
- **Decided:** Private binding compile checks use the real binary in a temporary
  source copy and reuse `target/`. A valid control precedes four failures checking
  identity, input/result signatures and private construction. The Python standard-
  library harness checks error codes and primary locations, so unrelated compiler
  failures cannot satisfy a negative case. Linux CI runs
  `python3 crates/drip-gui/tests/check_bindings.py` after workspace tests.

## Processing and responsiveness

- **Requirement:** The suggested Bayer pipeline is RAW → WB → highlights → RCD →
  camera to Rec.2020 → exposure → sigmoid → TIFF. Exposure is separate from tone
  mapping. Adapted algorithms retain notices, deviations and reference tests.
- **Decided:** Preserve negative sensor samples; normalization does not apply WB.
  Camera matrices use dcraw-style normalization without a separate CAT. Retain
  characterization with pixels; there is no scene/display order gate or WB-history
  restriction. Stronger calibration evidence needs a concrete consumer.
- **Decided:** Sensor processing precedes preview reduction, retaining clipping
  information at the cost of full-sensor passes. Reduction belongs to demosaic
  integration. One manual preview detail applies across the graph; export uses
  full detail. Navigation does not change processing resolution.
- **Decided:** Export writes through explicit actions and takes metadata through
  an optional edge, separating provenance from pixel interpretation. Strict JSON
  stores opaque frontend layout, with no prototype migration. Template inputs
  use ordinary parameters. The graph is the sole editable state.
- **Decided:** Use Rayon. The CubeCL trial was slower and harder to develop at
  host-valued node boundaries. Reconsider GPU execution only with measured
  whole-node gains, including transfers/allocations. Sub-300 ms latency is
  acceptable without further optimization.
- **Decided:** Evaluate all GUI consumers, including off-screen ones. The worker
  owns evaluation, preparation and large-image retirement. Coalesce edits, publish
  only current results, keep old views while working, replace them on current
  failure. Preview and export share decoded resources, without a normalized
  pyramid or retained full-detail export graph cache.
- **Decided:** Export captures the graph/resources before spawning and survives
  edits, invalidation or project replacement. Worker dispatch owns command
  ordering, cache lifetimes and action spawning; handlers own computation.
- **Decided:** Cross-camera tests use CC0 raw.pixls.us fixtures plus the original
  Sony photograph; Git/LFS owns integrity. Skip only unsupported LibRaw formats/
  decoders. Track unusable metadata separately in TODO. Consistency tests and
  plausible renderings do not establish colorimetric accuracy.

## Presentation and editing

- **Decided:** Use extended-linear BT.709 output with an SDR Rec.2020 bound and
  bounded-sRGB fallback. Explicit Wayland metadata avoids driver-defined white;
  macOS/Windows use native scRGB paths. Exact encoding/blending contracts live in
  [display](../crates/drip-gui/src/render/display.rs), and the wgpu patch/removal
  condition in THIRD_PARTY. HDR and physical output validation remain outstanding.
- **Decided:** Scopes inspect connected values without display-profile conversion:
  EV for channel exposure, exposure-independent u′v′ for chromaticity. Use black
  for scopes, middle grey for image previews.
- **Decided:** Preview offers normal, softproof and cyan gamut warning. Proofing
  shares profile settings/ICC conversion with export and runs on the worker.
  Softproof simulates bounded output; the LittleCMS gamut mask is approximate.
  Lab input improves sampling near black. Parallel ICC conversion disables the
  mutable LittleCMS pixel cache. Proof edits retain upstream computation.
- **Decided:** Preview interpolation is persisted, off by default. Canvas/pop-outs
  use nearest-neighbor or bilinear sampling over shared pixels/textures. Pop-out
  zoom/pan is local, anchored at the pointer, bounded by image edges, and expressed
  in render-target pixels accounting for desktop scale. Show detail/dimensions.
- **Decided:** Canvas is the main workspace. Parameter/view pop-outs are OS windows
  above it, arranged by the WM; no docking or persisted pop-out layout. Scale canvas
  text with its layer, accepting softness at large zoom. Idle windows should rest.
- **Decided:** GUI-owned state accessors centralize persisted layout keys and
  preview detail while preserving unknown JSON fields. Layout edits use typed
  fields through the shared edit path. Invalid/nonfinite positions and undersized
  views fall back locally; invalid preview detail rejects project replacement.
  The processing library keeps the UI payload opaque.
- **Decided:** `CanvasLayout` shares one frame's node/port geometry between drawing
  and navigation. Draw wires before nodes; graph edits affect next-frame geometry.
  Shared [editing](../crates/drip-gui/src/editing.rs) determines redraw/evaluation
  effects. Presentation-only edits do not reprocess images.
- **Decided:** Node IDs are stable; category and default name drive the sorted Add
  menu. Instances keep editable names. Technical help appears below the type ID
  in the main inspector only. Frontend labels explain backend contracts.
- **Decided:** Left-drag pans over background/nodes; right-drag moves nodes.
  Right-click opens Add or Rename/Delete by context. Ports navigate peers on left
  click and connect on right click. Invalid targets preserve wiring; Escape or
  background right-click cancels. Right-click disconnects the nearest hovered wire
  within six screen points, with controls/ports taking priority. See the complete
  [gesture table](../README.md#usage). Menus share lighter neutral fills/dark borders.
- **Decided:** Shared parameter controls use wheel edits and right-click Reset
  from schema defaults; sliders reject right-button value edits. Floats advance
  1% of range per line, integers one unit. Dropdowns advance in menu order and
  stop at endpoints. Fractional scrolling accumulates only while hovered; edits
  consume panel scroll. Require a current pointer position, including on leave
  frames, and clear remainders when it disappears.
- **Open:** Confirm AGPL `-or-later` versus `-only`; the former is currently recorded.

## Logging

- **Decided:** Frontends configure stderr `log`, defaulting to
  `warn,drip=info,drip_gui=info`. Kernels return errors without warning/error logs;
  UI status updates alone are not log events.

| Level | Events |
|-------|--------|
| Error | Failed explicit open/save/action, stopped worker, unusable display/window. |
| Warn | Newly failed live preview, display fallback/recovery, unavailable parenting. |
| Info | Startup/version, GPU/output selection, project open/save, action start/completion. |
| Debug | Evaluation/timing, resources, invalidation, window lifecycle, incomplete configuration and refused edits. |
| Trace | Cache hits, discarded requests, frame timings/skips and repaint scheduling. |

- **Decided:** Log each changed root preview failure once, after accepting its
  generation; reset clears history. Actions report failure at error level even
  for incomplete configuration and retain dependency origin. Include node/kind,
  detail, generation, window or destination/timing where relevant. Display
  selection owns its single fallback warning with the underlying reason.
