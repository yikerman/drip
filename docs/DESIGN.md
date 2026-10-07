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

## DAG type system

### Philosophy and goals

- **Requirement:** Suggest useful pipelines without prescribing module order.
  Check operation prerequisites, not aesthetics or workflow recommendations.
- **Requirement:** Verify pipeline contracts as formally as practical with
  contained complexity. Do not build a general compiler or theorem prover.
  Numerical laws remain obligations of trusted interpretations and kernels.
- **Requirement:** Define each node locally once. Generate descriptions, adapters,
  discovery, help and diagnostics from that declaration. Generated lookup data is
  acceptable; manually synchronized node/type/casting catalogues are not.
- **Requirement:** Support future processing families without requiring speculative
  framework primitives. Add contracts and intermediates when users need them.

### Case studies

- **Linearity and creative processing:** nonlinear RAW reconstruction can estimate
  linear signals. Sigmoid computes a nonlinear rendering but interprets its output
  as linear RGB. Darktable's modern [color balance RGB](https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/color-balance-rgb/)
  and [tone equalizer](https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/tone-equalizer/)
  likewise change physical relationships to the capture. Creative processing
  deliberately interprets the result y as new light coordinates: XYZ = M y.
  This neither undoes the edit nor weakens the definition of linearity.
- **Masks and pixel math:** a scalar field is not necessarily a weight in [0, 1].
  Complement and multiplication preserve that bound; addition need not. Masks may
  depend on pre- or post-operation data, as in darktable's [parametric masks](https://docs.darktable.org/usermanual/5.6/en/darkroom/masking-and-blending/masks/parametric/).
- **Filtering and geometry:** normalized nonnegative filters may preserve bounds
  with suitable boundary handling; signed filters do not inherit that promise.
  Masking a blur result differs from restricting its support. Equal dimensions
  do not establish alignment after crop or distortion. Lens correction combines
  geometry, channel sampling and gain, with different effects on contracts.
- **Calibration and noise:** CFA, black levels, gains, saturation and calibration
  must describe the actual samples. Scaling/interpolation can invalidate a noise
  model even when capture metadata is retained. Provenance is not applicability.
- **Multiple frames:** denoising, super-resolution, HDR and stitching may consume
  multiple paths and return one demosaiced image. Alignment/merging can stay inside
  the node; track each resource without mandating frame stacks or alignment maps.

### Requirements

- **Requirement:** A node computes outputs from explicit context, parameters and
  inputs. Computation is pure; caches do not change results. The dataflow is acyclic.
  External resources participate in dependency tracking and invalidation.
- **Requirement:** Storage layout and logical type are distinct. Three-channel
  fp32 storage can represent incompatible types, like metres and seconds sharing
  float storage. Capabilities classify interpretations through laws and operations.
  Immutable samples retain the instance data needed to interpret them.
- **Requirement:** `LinearRgb` means intended linear, colorimetric coordinates in
  a declared RGB basis with a mapping to XYZ D65. It says neither that its producer
  is linear nor that the values retain capture relationships. Creative nodes may
  establish this interpretation for their result without a separate user override;
  exposure history, noise statistics and bounds are additional promises.
- **Requirement:** Express per-input requirements, relationships among actual
  inputs, and output guarantees. Consumers request only what they need. Distinguish
  incompatible, unresolved and unestablished guarantees; metadata copying alone
  grants none. An unmet prerequisite blocks its consumer; ordering advice does not.
- **Requirement:** Compatibility, input binding and diagnostics share one
  definition. Validate external data at its boundary and resolve instance
  constraints before consuming it. Kernels trust validated inputs; capabilities
  do not imply repeated full-buffer scans.
- **Requirement:** Diagnostics identify node/port, expected and actual properties,
  and the unmet requirement. Keep incomplete configuration, incompatibility,
  processing failure and internal contract violations distinct across frontends.

### Declarations and capabilities

- **Decided:** `RawMat<C>` holds row-major fp32 samples and geometry;
  `RealMat<C, I>` adds an immutable interpretation witness. Mosaic, camera RGB and
  Rec.2020 use this construction. Other payloads implement `EdgeValue` locally;
  there is no central payload enum. See [image](../crates/drip/src/image.rs) and
  [value](../crates/drip/src/value.rs).
- **Decided:** Capability traits declare laws and operations. `LinearRgb` extends
  `Linearity + Colorimetry`; `Rec2020Rgb` adds a fixed basis. `#[capability]` uses
  Bevy trait projection and exposes annotated parents automatically.
  `#[interpretation(...)]` generates immutable, memoized dictionaries from typed
  implementations. Capabilities are object-safe and nongeneric; instance
  parameters live in interpretation data. No caller maintains a mutable registry.
- **Decided:** `#[node(...)]` accepts a function body or a signature ending in `;`.
  It generates a declaration identity (`foo_bar` → `FooBarNode`), `TypedNode<N>`,
  adapters and linked discovery. Metadata supplies stable ID, category, name,
  output names and optional checks/actions. Input names come from tuple bindings;
  Rust types supply the contracts. Duplicate IDs are declaration errors.
- **Decided:** Macro inputs use `&T`, `MatRef<'_, C, dyn Capability>`, or `Option`
  of either. Qualified paths work; wrapper aliases are not resolved. `&T` becomes
  an exact logical-type requirement; `MatRef` requires layout plus capability.
  Handwritten `NodeDeclaration` implementations use the same executor.
- **Decided:** `NodeDeclaration` owns parameter/input/output types, typed checks,
  actions and an optional kernel. Kernels return `Result<Outputs, KernelError>`
  directly. Kernel-less declarations have no outputs: preview/scopes are input
  consumers; export supplies an explicit action. Headless tools can load and
  validate their contracts without executing GUI preparation.
- **Decided:** `Parameters` derives persisted keys, defaults, bounds, typed access
  and field help; flattening reuses schemas without changing project keys.
  `Choice` derives finite enum strings and schemas, checked against field types
  and defaults at compile time. Kernels receive typed parameters, never an
  independent schema or raw map. Node Rustdoc and declaration reference links
  supply inspector help; implementation rationale stays in ordinary comments.

### Preservation and runtime checks

- **Decided:** Outputs have fixed logical types or `Preserved<input_index>`.
  Preservation expresses `RealMat<C, I> -> RealMat<C, I>` after a capability borrow
  erased concrete I. The graph needs that relationship before executing pixels;
  a produced-value inspection would defer connection errors until evaluation.
  Runtime checks retain both type and the input's interpretation witness, including
  calibration data. They reject preserving a different input witness even when
  both have the same Rust type. See [ports](../crates/drip/src/ports.rs).
  A capability alone does not establish preservation: exposure requires
  `ScaleInvariant: LinearRgb`, meaning *all* interpretation promises survive
  positive uniform scaling. `Rec2020Rgb` alone grants no such law. Sigmoid instead
  establishes fresh Rec.2020 output and drops refinements. Keep this fixed-or-
  preserved model explicit; generic inference would still need runtime contracts
  while adding macro complexity. No unification or symbolic expressions.
- **Decided:** A connection checks endpoints, single-source input replacement,
  cycles and known type requirements. Resolve preserving chains and recheck
  downstream edges transactionally; incompatibility rolls back the edit. An
  unconnected preserving chain has a pending type. Editing may retain pending
  connections, but they do not authorize evaluation.
- **Decided:** Runtime binding uses the same `InputRequirement::check` as graph
  validation. Node-local typed predicates check actual units, basis parameters,
  geometry or other relationships before kernels/actions. TypeId alone does not
  compare instance data. Optional inputs allow absence, not failure or mismatch
  of a connected source. Errors carry the failing node and named port/check.
- **Decided:** Evaluation caches output values and errors by dependency stamps,
  parameters and resolution, without payload equality. RAW/ICC reads share explicit
  resource snapshots. Replacing the evaluator invalidates its caches; existing
  forks retain their snapshot. Same-path changes require manual invalidation.
- **Decided:** Rust checks signatures and trait implementations; generated adapters
  check erased bindings and runtime relationships. Soundness still depends on
  truthful laws, correct kernels and external-data validation. Neither reflection
  nor macros prove numerical behavior or calibration.
- **Open:** Add richer interpretations, constraints and widgets with their first
  consumers. Masks, geometry and multi-path fusion need no speculative framework.
  Windows/macOS discovery and Rust 1.92 execution remain validation work.

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
