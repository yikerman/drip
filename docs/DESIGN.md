# Design intent

Decisions and rationale; detailed contracts live beside the code. Unfinished work
is in [TODO](../TODO.md), validation evidence in [PROGRESS](PROGRESS.md), and
algorithm credits in [THIRD_PARTY](../THIRD_PARTY.md).

Statuses: **requirement** = user goal; **decided** = agreed direction;
**tentative** = proposal; **open** = unresolved.

## Direction and boundaries

- **Decided:** v0.1 work proceeds through production-pipeline validation, editing
  UI/UX, SpyderX color verification, then packaging. CLI expansion and speculative
  optimization follow those priorities.

- **Requirement:** Processing, graph validation, color management and persistence
  belong in the GUI-independent library. Support Linux/Wayland, macOS and Windows
  with correct display color on each compositor. Linux/Wayland is the primary
  development environment; a Linux-only color path does not qualify a replacement
  GUI. X11 is unsupported.

- **Requirement:** Use LibRaw for decoding and LittleCMS for ICC transforms.
  The minimal owned-data [LibRaw binding](../crates/drip-libraw/src/lib.rs) keeps
  version-dependent native layouts behind a C shim.

- **Decided:** Cross-camera tests use CC0 raw.pixls.us fixtures and the original
  Sony photograph. Skip only LibRaw's unsupported-format/decoder errors;
  fixture integrity belongs to Git/LFS. Track unsupported metadata/unpacking
  cases in TODO and keep the active corpus limited to renderable samples.
  Numerical references, pipeline contracts and visual review provide different
  evidence; a plausible rendering is not a colorimetric ground truth.

- **Requirement:** Group source by semantic responsibility, not file size.
  Keep one computational kernel source across backends; portability and ease of
  development take priority over peak throughput.

- **Decided:** Nodes own their schemas, adapters and kernels. Demosaicing and
  scopes form local families; GUI rendering and shared node controls have their
  own homes. Keep cohesive implementations together.

## DAG type system redesign

### Philosophy and goals

- **Requirement:** Suggest useful pipelines without prescribing module order.
  Check the actual prerequisites of each operation and explain incompatibilities;
  distinguish these from workflow recommendations and aesthetic choices.

- **Requirement:** Establish pipeline correctness as formally as practical, with
  contained implementation complexity. Use current code and modern processing
  workflows as requirements, not as a reference architecture. Avoid a general
  compiler or theorem prover. Guarantees remain conditional on trusted kernels
  satisfying their declared laws; reflection cannot prove numerical algorithms.

- **Requirement:** Define a node locally once. Generate mechanical descriptions,
  adapters, discovery, contract help and consistent diagnostics from that source.
  Avoid hand-maintained node lists and parallel compatibility/casting definitions.
  Mathematical requirements and output guarantees still need authored definitions.
  Any registration implied mechanically by declarations must be generated;
  generated lookup data is an implementation detail, not an authoring obligation.

- **Requirement:** Remain extensible beyond the present RAW pipeline. Add value
  families and contracts locally; expose algorithm intermediates only when users
  need to connect them. Framework complexity should follow public dataflow needs.

### Case studies

These cases identify distinctions the design must express, without mandating
particular types, APIs or implementations.

- **Linearity and color interpretation.** RAW reconstruction can estimate linear
  sensor signals using nonlinear algorithms. Sigmoid produces linearly encoded
  display RGB after a nonlinear rendering transform. A nonlinear encoding can
  still have a defined mapping to XYZ D65. Sample encoding, algorithm linearity
  and proportionality to original exposure are therefore different promises.

- **Creative processing.** Darktable's modern
  [color balance RGB](https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/color-balance-rgb/)
  and [tone equalizer](https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/tone-equalizer/)
  perform nonlinear or locally varying edits within a scene-referred workflow.
  These operations really do break physical/mathematical relationships to the
  original signal. Creative processing deliberately reinterprets the resulting
  numbers as new light values in the declared color space. For output y and its
  RGB-to-XYZ matrix M, the interpretation is XYZ = M y; it does not undo the
  creative operation. This is an explicit output contract, not a weakened
  definition of linearity or an unresolved assumption.

- **Masks and pixel math.** A scalar field need not be a blend weight in [0, 1].
  Complement and multiplication preserve bounded mask weights; addition need not.
  Arbitrary pixel math may retain geometry while losing range or measurement
  guarantees. A mask may depend on an operation's input or its processed output
  before blending, as in darktable's
  [parametric masks](https://docs.darktable.org/usermanual/5.6/en/darkroom/masking-and-blending/masks/parametric/).

- **Filtering and geometry.** A normalized nonnegative filter can preserve scalar
  bounds with compatible boundary handling; a signed filter cannot claim the
  same rule. Masking a blur's result differs from restricting its sample support.
  Matching buffer dimensions do not establish image/mask alignment after crop or
  distortion. Lens correction combines spatial remapping, channel-dependent
  sampling and photometric gain, which have different effects on contracts.

- **Calibration and noise.** CFA layout, black levels, channel gains, saturation
  and characterization must describe the actual samples. A noise model may cease
  to apply after scaling, interpolation or creative edits, even when capture
  metadata remains available. Provenance alone is not applicability evidence.

- **Multi-frame processing.** Denoising, super-resolution, HDR and stitching may
  take multiple file paths and produce one demosaiced image. Alignment, merging
  and their intermediates can stay inside the node. Track every resource consumed;
  frame collections and alignment maps need not become framework primitives.

### Requirements

- **Requirement:** A node computes outputs from explicit context, parameters and
  inputs. Computation is pure; caches do not change results. The dataflow is a DAG.
  External resources participate in dependency tracking and cache invalidation.

- **Requirement:** Represent storage separately from its interpretation and
  guarantees. Cover scalar/three-channel rasters, structured metadata and other
  typed values without a closed list of all future payloads. Retain metadata
  needed to interpret immutable samples with those samples.

- **Requirement:** The DAG's primary types are logical/semantic types. A
  three-channel `f32` matrix is a storage representation shared by potentially
  incompatible types, just as metres and seconds can both use floating-point
  storage. Capabilities classify semantic types through their documented laws
  and operations. Shared representation does not establish type compatibility;
  semantic distinctions are not merely predicates on otherwise interchangeable
  buffers. Logical types and their parameters may be represented at runtime;
  they need not all become statically monomorphized Rust types.

- **Requirement:** `LinearRgb` means RGB data intended to be linear and
  colorimetric. Its samples are interpreted as linear light coordinates in a
  declared RGB basis with a defined mapping to XYZ D65. This is a semantic
  contract of the data, not a claim that the producing operation is linear or
  that the values preserve the original capture. Creative reinterpretation can
  deliberately establish this contract for newly computed values.

- **Requirement:** Express per-input prerequisites, relationships between inputs
  and output guarantees. Consumers request only what their operation needs.
  Distinguish known incompatibility from facts awaiting external data and from
  semantic properties that have not been established. Do not silently grant
  guarantees after arbitrary processing or metadata copying.

- **Requirement:** Distinguish preserving a physical relationship from explicitly
  assigning an interpretation to new values. A nonlinear creative node may declare
  linear RGB output by interpreting its result as-is; that declaration is part of
  the node's meaning and needs no separate user override. It does not establish
  preservation of original exposure relationships, noise statistics or bounds.
  Check downstream consumers against the declared output interpretation and the
  additional guarantees actually established.

- **Requirement:** Use one definition for compatibility, kernel input binding and
  its diagnostic. Validate external data at its boundary and resolve outstanding
  instance constraints before invoking consumers that rely on them. Internal
  kernels may trust validated inputs; avoid repeated full-buffer validation.

- **Requirement:** Return structured diagnostics containing node/port context,
  expected and actual properties and the unmet requirement. Editing, loading,
  evaluation and frontends share them. Separate invalid inputs, unresolved facts,
  processing failures and internal contract violations. Suggestions must not be
  presented as proven incompatibilities.

- **Decided:** Generate typed adapters and metadata, resolve fixed/preserving
  output types over the DAG, and run declared value predicates when their inputs
  exist. Recheck dependent contracts after edits. Keep numerical laws on traits
  and nodes; do not infer them by analyzing kernel bodies.

- **Decided:** An unmet prerequisite blocks execution of its consumer; ordering
  advice does not. Defer checks awaiting decoded or computed facts, without
  treating them as satisfied. A guarantee with no establishing evidence or
  pending producer remains unavailable. Recompute contracts when their parameters,
  inputs or resource dependencies change.

- **Open:** Identify additional capture/calibration guarantees when implementing
  consumers that need them. Existing scene/display markers and the choice not to
  track WB history are implementation precedents, not redesign constraints.

### Implementation method

- **Decided:** `RawMat<C>` owns row-major fp32 samples and geometry;
  `RealMat<C, I>` pairs that storage with an immutable interpretation witness.
  `Mosaic = RealMat<1, SensorMosaic>`, camera RGB and Rec.2020 RGB share this
  construction. Structured metadata implements `EdgeValue` locally. Adding a
  payload does not require editing a central type enum or descriptor list.

- **Decided:** Capability traits contain laws and operations. `LinearRgb`
  extends `Linearity + Colorimetry`; `Rec2020Rgb` additionally promises the fixed
  Rec.2020 basis. `#[capability]` uses Bevy's trait projection and automatically
  exposes annotated parent capabilities. `#[interpretation(Capability, ...)]`
  builds an immutable local dictionary from typed implementations. A memoization
  cache avoids rebuilding dictionaries; no caller maintains or mutates a type
  registry. Capabilities are object-safe and nongeneric; instance parameters
  belong in interpretation data.

- **Decided:** `#[node(...)]` on a typed function derives input names from its
  tuple bindings, adapters from argument/return types, and a `linkme` discovery
  entry. It also declares stable ID, category, display name, output names and
  optional checks/actions. `Read<T>` requires the exact logical type;
  `MatRef<C, dyn Capability>` requests a layout and interpretation capability.
  Optional ports are `Option` of either form. `NodeKernel` remains available for
  handwritten extensions; generated and handwritten nodes use the same executor.
  Both derive their schema from `NodeKernel::Parameters`; evaluation, actions
  and checks receive that type after conversion at the erased adapter boundary.
  There is no independent schema argument or raw-map kernel interface.
  Duplicate stable IDs are declaration errors.

- **Decided:** Generated node constants are `TypedNode<P>`, retaining their
  parameter struct type. The wrapper owns its immutable `NodeKind`, constructs
  its schema from `P` and requires the same `P` as the kernel. Graph APIs borrow
  the descriptor through deref coercion; heterogeneous lists use `.kind()`.
  Parameter structs exposed through public declarations are public, while fields
  may stay private behind domain-specific methods.

- **Decided:** Parameter structs derive `Parameters`.
  `#[param(...)]` fields generate JSON keys, defaults, bounds and typed access;
  flattening shares profile/scope schemas without changing project keys.
  Rustdoc supplies technical field descriptions; Bevy reflection is confined to
  interpretation evidence, with no unused parameter-reflection layer. Node
  function Rustdoc supplies operation help; `references = [(label, url), ...]`
  on the same declaration supplies links. The frontend renders this metadata
  with generated controls and port-contract help, without a separate help table.

- **Decided:** Finite parameter choices derive `Choice` on unit enums. Each
  variant declares its persisted string once with `#[choice("name")]`; the
  derive generates JSON conversion and `variant.schema()` for field defaults.
  Kernels match enums exhaustively. Generated parameter declarations check named
  field categories and the complete choice schema at compile time, including
  default membership, so an admitted JSON choice is readable by its field.

- **Decided:** `Evaluated<Outputs, V>` declares headless presentation through
  its return type. `V = ()` (the default) has no view; concrete image/scope results
  and `View` have one; `Option<V>` permits an absent result while retaining its
  presentation surface. The sealed `Presentation` trait supplies both conversion
  and availability, so there is no separately maintained view flag. The GUI
  provides a generic viewer even before successful evaluation. Custom
  GUI bindings use `linkme` declarations beside their implementations; a generated
  lookup rejects duplicate node IDs. No central node-to-widget table is maintained,
  and no GUI dependency enters the library. Each binding independently overrides
  typed controls and/or `NodeView` (body, sizing and pop-out windows).
  Omitted controls use the parameter schema; an omitted view uses the node's
  declared generic presentation. Custom controls therefore retain a generic
  viewer without forwarding its methods.

- **Decided:** Custom controls bind `Controls<P>` to `TypedNode<P>` through one
  constructor; its stored fields are private to a sibling module, preventing
  node-specific modules from bypassing the type check with literals. Callbacks
  receive `ControlCx<P>` with typed parameter reads and schema controls for
  validated edits. Reads occur when requested, including after same-frame edits. Erasure
  happens only inside the binding adapter. This checks structural parameter
  compatibility, not a callback's intent when two nodes share a parameter type.

- **Decided:** Outputs are fixed logical types or `Preserved<input_index>`.
  Preservation retains the concrete interpretation and its instance witness;
  accepting a capability alone never establishes preservation. The adapter
  verifies that a preserving result came from its declared input, even when
  two inputs share a Rust TypeId. A node that breaks stronger guarantees must
  explicitly construct a weaker/new output interpretation. Sigmoid establishes
  Rec.2020 output; exposure requires `ScaleInvariant: LinearRgb` to preserve
  the full input interpretation. This explicit closure law prevents retaining
  arbitrary bounded/calibrated refinements under exposure. Plain Rec.2020 has
  that law; `Rec2020Rgb` alone deliberately does not imply it. A refined type can
  be explicitly weakened by a node that constructs its base interpretation.

- **Decided:** Connecting checks endpoints, single-source input replacement,
  cycles and all presently known type requirements. A candidate edit resolves
  preserving chains and rechecks downstream edges before committing; a known
  incompatibility rolls the whole edit back. Unconnected preserving chains have
  a pending output type. Pending connections may be stored while editing, but
  do not establish compatibility or authorize evaluation.

- **Decided:** Runtime binding uses the same `InputRequirement::check` as graph
  validation. Named typed preconditions check relationships such as equal units,
  basis parameters or geometry once actual inputs exist, before computation or
  actions. TypeId identifies a logical constructor, not equality of its runtime
  parameters. These predicates are ordinary node-local Rust functions; their
  declarations appear in inspector help as requirements checked at evaluation.
  No symbolic inference, equation solver or kernel-body analysis is involved.

- **Decided:** The evaluator retains dependency-stamp caching and explicit
  resource snapshots. RAW decoding and ICC file reads use `EvalContext`'s
  shared resource store. Replacing the evaluator invalidates resource and result
  caches; already forked readers keep their snapshot. Same-path file changes
  require explicit invalidation. Parameters/source changes propagate through
  downstream stamps and rerun applicable preconditions.

- **Decided:** Type mismatches contain expected/actual interpretation or
  capability and the reason. Graph errors name the affected port, including
  downstream consequences; evaluation errors retain input names and node context.
  Missing inputs, pending types and failed value relationships stay distinct from
  successful checks. Frontends format these contracts, rather than maintaining
  a second compatibility table.

- **Decided:** Soundness is conditional on correct interpretation implementations,
  honest kernel output laws and external-data validation. Rust verifies signatures
  and capability implementations; generated adapters verify erased bindings and
  runtime relationships. Neither Bevy nor these macros proves numerical kernels,
  physical calibration or docstring axioms. No additional full-buffer scan is
  implied by a capability.

- **Open:** Add richer interpretation families, parameter widgets and predicates
  alongside their first real consumers. Masks, geometry-changing operations,
  multi-path RAW fusion and stitching need no mandatory frame-stack representation.
  Linker discovery has been exercised on Linux; Windows/macOS execution and an
  actual Rust 1.92 build remain verification work, rather than new type machinery.

## Processing and graph contracts

- **Decided:** Adopt the Rust reflection/macro implementation described above.
  It preserves the native image kernels and GUI color path. Earlier CLR/GUI
  experiments below remain independent feasibility evidence.

- **Open:** Evaluate a CLR rewrite; no migration is decided. CLR interface
  assignability is an alternative to the generated Rust capability projections in
  `value`/`ports`. Derive port metadata and invocation from the same typed kernel
  signature. Optionality, parameter constraints, stable serialized IDs and pixel
  semantics remain application contracts. F# fits the functional core; C# is a
  candidate for native interop and the GUI boundary. Published pixel buffers must
  remain immutable: GC does not replace Rust's aliasing and thread-safety checks.

- **Tentative:** Gate a CLR migration on a small presentation prototype and
  representative numerical/latency comparisons. Dear ImGui with controlled
  presentation is the strongest CLR GUI candidate from the local probes: image
  values and deliberately linearized UI composition survived FP16 rendering.
  Stock GTK 4.22.5 failed local color negotiation and lacks equivalent macOS
  wide-gamut output. Stock SDL3/Mesa preserved pixels but omitted wider Wayland
  target primaries, leaving those pixels outside the declared color volume.
  These are prototype findings, not adoption or cross-platform validation.

- **Tentative:** If a CLR GUI proceeds, retain SDL_GPU's rendering backends and
  add a narrow Wayland presentation extension. A local SDL 3.4.16 prototype
  starts opted-in windows with FP16 Vulkan passthrough, preserving that choice
  across recreation; a separate adapter owns the explicit color description.
  The SDL delta is 30 added/four replaced lines, not the total color-management
  implementation. Stock Metal/D3D12 configuration appears suitable for managed
  output by source audit. Windows still needs Advanced Color state detection,
  HDR-only white scaling and legacy ICC output. No backend rewrite or migration
  is approved by these results; native platform tests and failure handling remain.

- **Open:** A CLR presenter must own the complete output contract: explicit
  Wayland gamut/luminance metadata without competing driver declarations,
  Windows Advanced Color scRGB with white-level handling or legacy ICC output,
  and tagged extended-linear Metal/Core Animation output. Track display/profile
  changes and keep UI and images in the same declared encoding. Physical output
  and Windows/macOS execution remain unverified. Dear ImGui automatic viewports
  are unavailable through the tested SDL Wayland backend; Drip pop-outs need
  application-owned windows. Prototype linear UI blending does not change the
  existing egui blending decision.

- **Requirement:** Typed input/output tuples are the source of connection and
  evaluation contracts. Consumers request the weakest meaningful capability;
  see [image laws](../crates/drip/src/image.rs),
  [port adapters](../crates/drip/src/ports.rs) and
  [erased values](../crates/drip/src/value.rs).

- **Decided:** Use linear/colorimetric interpretation and explicit RGB basis
  requirements, without a scene/display order gate. Retain camera characterization
  with pixels. WB-history restrictions remain absent; stronger calibration
  evidence belongs with consumers that actually require it.

- **Decided:** Evaluation has no output side effects; file writes are explicit
  actions. Preserve dependency-stamp caching, including preview-level
  invalidation, without requiring payload equality. Cache lifetimes and optional
  input failure semantics are documented in [eval](../crates/drip/src/eval.rs).

- **Requirement:** The initial Bayer pipeline is RAW → WB → highlights → RCD →
  camera to Rec.2020 → exposure → sigmoid → TIFF. Exposure remains separate from
  tone mapping. Adapted algorithms retain local notices, deviations and tests.

- **Decided:** Preserve negative sensor samples and avoid implicit WB during
  [normalization](../crates/drip/src/nodes/raw.rs). Use dcraw-style camera matrix
  normalization without a separate CAT; see [color](../crates/drip/src/color.rs).

- **Decided:** Sensor processing precedes preview reduction, preserving clipping
  information at the cost of full-sensor passes for small previews. Reduction
  belongs to [demosaic integration](../crates/drip/src/nodes/demosaic/mod.rs),
  keeping kernels independent of evaluation detail.

- **Decided:** Export metadata travels through an explicit optional edge, keeping
  provenance separate from image semantics. TIFF/profile behavior is defined in
  [export](../crates/drip/src/nodes/export.rs).

## Responsiveness

- **Decided:** Use Rayon. The CubeCL trial passed correctness checks but was
  slower and harder to develop at host-valued node boundaries. GPU work needs
  measured whole-node benefit, including transfers and allocation.

- **Requirement:** One manual preview detail applies across the graph; export
  uses full detail. Navigation must not change processing. Latency below 300 ms
  is acceptable without further optimization.

- **Decided:** Evaluate all GUI nodes so off-screen results and errors stay
  current. Keep CPU evaluation, preparation and large-image retirement off the
  UI thread. Coalesce edits, publish only the latest requested preview, retain
  old views while working, and replace them on current failure.

- **Decided:** Export captures the graph at the click and survives later edits
  or project replacement. Preview/export share decoded files, without retaining
  a normalized pyramid or a full-detail export graph cache. Scheduling lives in
  [worker](../crates/drip-gui/src/worker.rs).

- **Decided:** Current prototype resource costs are acceptable. Robust saves,
  close protection, cancellation and memory budgets remain deferred in TODO.

- **Decided:** `worker::serve` owns command dispatch, cache lifetimes and action
  thread spawning. Preview evaluation/preparation and action execution have
  private handlers; one-line reset/invalidate, collect and shutdown operations
  stay in the dispatcher. Fork the action evaluator before spawning to preserve
  reset/invalidation ordering. Prepared-allocation retention and panic boundaries
  remain unchanged.

## Presentation and editing

- **Decided:** `CanvasLayout` owns one frame's node and port geometry in graph
  coordinates. Drawing and navigation share its lookups; graph edits affect the
  next frame's geometry. `Editor::show` delegates wire drawing/hit-testing,
  individual node UI and connected-peer menus to private helpers. Wires precede
  nodes, navigation follows the node loop, and pending-wire cleanup follows
  deletion. Widget IDs and immediate `Frame::edit` behavior remain unchanged.

- **Decided:** Node kinds declare a stable `id`, an internal `category` key and
  a capitalized default `name`. Each graph node owns its editable name, changed
  through the existing Rename action or inspector field. Serialized type IDs and
  the file's `label` field retain their existing meaning and values.

- **Decided:** The canvas Add node menu groups kinds by category and sorts by
  category, name and ID. Internal category keys are shown directly. Selection
  returns a kind to the editor, which owns the graph edit; no separate catalog
  state is stored. Parent menus and submenus inherit one shared theme style.
  A dedicated node picker remains deferred.

- **Decided:** Parameter controls share conventional gestures in the inspector,
  pop-outs and template inputs. Scrolling a slider changes a float by 1% of its
  range per line, or an integer by one, and consumes the panel's scroll input.
  Right-click opens Reset to default, using `ParamKind::default_value`; paths
  expose it on their filename so the browse button retains its normal action.
  The parameter name's existing menu shares the same reset action.
  Reset uses a menu to avoid conflicts with controls' single-click behavior.
  Slider setters reject right-button edits before painting so opening the menu
  preserves both the stored value and its display.
  Values remain in the graph. Only fractional scroll steps live temporarily in
  egui memory while a slider is hovered.

- **Decided:** Canvas and node gestures separate navigation from modification.
  Left-drag pans the whole canvas even when started on a node. This decision
  covers only the canvas background and node body.

  | Context | Left click | Left drag | Right click | Right drag |
  |---------|------------|-----------|-------------|------------|
  | Empty canvas | Clear selection | Pan canvas | Open Add node menu | No action |
  | Node | Select and show inspector | Pan canvas | Open Rename / Delete menu | Move node |

  The mouse wheel zooms around the pointer in both contexts. Rename selects
  the node and focuses its existing label field in the inspector.

- **Decided:** Input and output ports share navigation and connection gestures.
  Left-click lists connected peers; choosing one selects its node and pans it
  into view only when needed. Right-click starts or completes a connection from
  either direction. Invalid targets preserve the pending wire and current graph;
  Escape or right-click on the background cancels. Port inspection stays local
  to these menus and type tooltips; the sidebar retains node selection.

- **Decided:** Right-click directly disconnects the hovered wire. A six-screen-point
  hit radius follows the drawn curve, with nearest-wire selection at crossings.
  Hit testing uses only canvas background input so nodes, ports and controls take
  priority. Hover thickens the targeted wire and outlines node bodies. The full
  gesture table and usage instructions are in [README](../README.md#usage).

- **Decided:** Navigation and right-click menus use a lighter neutral fill and a dark border
  to separate them from the middle-grey workspace; styling is shared in
  [theme](../crates/drip-gui/src/theme.rs).

- **Decided:** Use one extended-linear BT.709 output pipeline with an SDR Rec.2020
  bound and bounded-sRGB fallback. Explicit Wayland tagging avoids driver-defined
  reference white; macOS/Windows use native scRGB. Encoding, blending and platform
  contracts are in [display](../crates/drip-gui/src/render/display.rs); the wgpu
  patch and removal condition are in THIRD_PARTY. HDR remains deferred.

- **Decided:** Scopes inspect connected data without a display-profile transform:
  EV for channel exposure, exposure-independent u′v′ for chromaticity. Their
  [kernels](../crates/drip/src/nodes/scopes/density.rs) and
  [drawing](../crates/drip-gui/src/render/node_views.rs) define the coordinates
  and annotation colors. Scopes use black; image previews use middle grey.

- **Decided:** Preview stores an `interpolation` parameter, off by default for
  discrete pixel inspection. Both the node and pop-out use nearest-neighbor
  sampling when off and bilinear sampling when on, for enlargement and reduction.
  Sampling is presentation metadata; prepared pixels and GPU textures remain
  shared across sampling choices.

- **Decided:** Preview offers normal, softproof and gamutcheck modes, sharing
  profile settings and ICC conversion with export. Softproof simulates bounded
  output; gamutcheck marks out-of-gamut colors in cyan. Both run on the worker
  and use the existing display path. Proof edits leave upstream processing cached.

- **Decided:** Feed Lab to LittleCMS's gamut checker to improve sampling near
  black. Its boundary remains approximate. Preview and export share parallel ICC
  conversion through Rayon, with LittleCMS's mutable pixel cache disabled.

- **Decided:** The canvas is the main workspace. Parameters and views can pop
  out into OS windows above the main window; the WM arranges them. No docking or
  persisted pop-out state. Scale canvas text with its layer to avoid atlas churn,
  accepting softness at large zoom; idle windows should not redraw continually.

- **Requirement:** Technical node help appears below the type ID in the main
  inspector, outside pop-outs. Frontend labels describe backend contracts rather
  than redefining compatibility; see [node UI](../crates/drip-gui/src/node_ui/).

- **Decided:** Preview pop-outs own temporary zoom and pan state. Fit follows
  window size; percentages map rendered image pixels to render-target pixels,
  accounting for the window's current desktop scale. Show rendered dimensions
  and reduction explicitly; zoom does not change processing detail. Wheel zoom
  anchors at the pointer, left drag pans, and image edges bound navigation.

- **Decided:** All dropdowns share wheel navigation in menu order: down selects
  the next option, up the previous, stopping at either end. Dropdowns and sliders
  share fractional scroll accumulation and consume scrolling over the control.

- **Decided:** The graph is the sole editable state. Shared
  [editing](../crates/drip-gui/src/editing.rs) determines redraw/evaluation effects;
  presentation edits do not reprocess images.

- **Decided:** Use strict, human-readable JSON with opaque frontend layout and
  no prototype migration. Template inputs use ordinary per-node parameters,
  avoiding separate binding machinery; see [project](../crates/drip/src/project.rs).

- **Open:** AGPL-3.0-or-later is recorded, but the original `-only` versus
  `-or-later` choice remains unconfirmed.

- **Decided:** Image zoom and control wheel edits require a hovered response with
  an available pointer position. egui can retain hover hit-testing on the frame
  the pointer leaves; stop edits then and clear control scroll remainders.

## Logging

- **Decided:** Keep `log` with frontend-owned stderr configuration. Default to
  `warn,drip=info,drip_gui=info`; library kernels return errors without reporting
  them at warning/error level. UI status updates do not imply log events.

| Level | Events |
|-------|--------|
| Error | Failed explicit open/save/action, stopped worker, unusable display/window. |
| Warn | Newly failed live preview, display fallback/recovery, unavailable parenting. |
| Info | Startup/version, GPU/output selection, project open/save, action start/completion. |
| Debug | Evaluation outcomes/timings, resource loads, cache invalidation, window lifecycle, incomplete configuration and refused edits. |
| Trace | Cache hits, coalesced/discarded requests, frame timings/skips and repaint scheduling. |

- **Decided:** Missing configuration is typed separately from failed processing.
  Report each changed root failure only after accepting the current preview;
  cached errors and upstream symptoms do not repeat warnings. Project reset
  clears this history. Explicit actions report failure at error level regardless
  of category, retaining the originating node for failed dependencies.

- **Decided:** Include node/kind and preview level in computation records,
  generation in worker records, window ID in presentation records, and destination
  and elapsed time in action records. Display selection owns the single fallback
  warning, including the underlying protocol/capability reason.
