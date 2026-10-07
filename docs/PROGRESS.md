# Progress handoff

Current outcomes and verification limits. Decisions are in [DESIGN](DESIGN.md),
unimplemented work in [TODO](../TODO.md); older experiments remain in Git history.

## 2026-10-07: Persistent GUI binding compilation tests

- Retained a valid binding and four rejected bindings in `drip-gui/tests`, with
  a standalone harness that compiles the real GUI source in a temporary workspace.
  It checks error codes and primary source locations, so unrelated build failures
  cannot satisfy a negative test. The identity case uses nodes with the same
  parameter/input types to distinguish identity from structural compatibility.
- Added the suite to Linux CI after workspace tests and removed the completed
  coverage follow-up from TODO. The valid probe and all four negative cases pass.
  Strict workspace Clippy, Rust formatting and diff whitespace checks passed.
  Production code is unchanged from the reviewed rewrite.
- Verified the harness runs from `/tmp`: source probes are temporary and Cargo
  reuses the repository's `target/`. Python caches are ignored. Retained Python's
  standard-library JSON parsing; a Bash version would need an additional parser
  to preserve diagnostic checks.

## 2026-10-07: Independent Codex and Claude Code reviews

- Both reviews found no blocking correctness issues or substantial
  overengineering. Codex inspected the rewrite, including untracked GUI modules,
  and independently passed eight declaration tests and five worker tests.
- Claude Code (Opus 5.5) completed a focused review of declarations, macros,
  typed GUI adapters, evaluation and worker ownership/caching after its broader
  review call timed out. It did not run tests or review every numerical module.
  Its observations concerned bounded image-memory retention, single-resolution
  caching and optional cleanup. Input observation despite a failed target kernel
  is intentional and covered by the declaration regression test.
- Tracked the actionable coverage follow-up: preserve the four isolated negative
  GUI compilation probes as repository tests. No production code changed during
  these reviews; neither review establishes native-window or platform coverage.

## 2026-10-07: Computational and GUI node separation

- Replaced `Evaluated` with direct output tuples and made `NodeDeclaration` the
  shared contract for optional kernels, actions and input consumers. Semicolon
  declarations retain schemas, help and ports without computational bodies.
  Typed handles now retain declaration identity as well as parameter/input types.
- Moved proofing, gamut checking, histogram and density preparation into GUI node
  modules alongside their presentation types and drawing. Local `#[gui_node]`
  implementations generate discovery. Default controls remain independent of
  preparation and views; prepared values use trait dispatch rather than an enum.
- Added typed input observation and a GUI preparation cache, including cached
  failures. Export and shared ICC conversion remain in the library. Saved node
  IDs, parameters and port contracts are unchanged. Moved numerical tests with
  GUI algorithms and retained headless processing coverage.
- Independent review found and resolved redundant input checks/observed-kernel
  execution, lost worker allocation ownership on reset, unchecked declaration
  signature positions, and underscore-name macro panics. A second review found
  no remaining production blockers. Accepted GUI snapshots retain generations.
- Validation: workspace tests passed 173 tests, including eight compile-fail
  doctests, with the GPU test ignored by default; the GPU color-reference test
  passed separately. Four isolated negative GUI builds rejected mismatched node
  identity, input signature, presentation type and private binding construction.
  Strict workspace Clippy, formatting and diff whitespace checks passed.
  Warning-free Rustdoc passed with `drip-cli` excluded: its `drip` binary collides
  with the library's documentation output path in a whole-workspace doc build.
  Windows/macOS and the declared Rust 1.92 minimum were not exercised.

## 2026-10-07: Preservation contract rationale

- Clarified `Preserved` beside its definition and in DESIGN: it expresses a
  same-interpretation input/output relationship through type erasure, enables
  connection-time checking, and retains interpretation data as well as its type.
  Recorded why the fixed-or-preserved model stays explicit instead of parsing
  general generic signatures. No behavior changed; diff whitespace check passed.

## 2026-10-07: GUI node boundary design

- Recorded the agreed separation of node declarations, computational kernels,
  explicit actions and typed GUI implementations. Kernels will return tuples
  directly; preview and scope preparation will move to the GUI with its own
  results and cache. GUI modules correspond by responsibility, not identical trees.
- Added a short headless-library boundary notice to AGENTS.md and replaced
  superseded presentation decisions in DESIGN. Migration is tracked in TODO;
  implementation is unchanged. Documentation-only change; checked diff whitespace.

## 2026-10-06: Maintainability fixes

- Unified handwritten and generated nodes on `NodeKernel::Parameters`: it supplies
  the schema and the values passed to evaluation, actions and checks. Removed the
  independent schema argument, macro conversion wrappers and unused raw getters;
  migrated handwritten test kernels to typed fields.
- Parameter reads deserialize borrowed JSON without cloning it first. Removed
  Bevy documentation reflection and the redundant test dependency; node/field help
  still comes from the declaration macros. Port roles now use an enum, with labels
  derived separately from hover behavior.
- Updated the design and dependency credits and removed the completed review
  items from TODO. Workspace tests passed 168 tests, including eight compile-fail
  docs; the GPU-dependent test remains ignored. Strict workspace Clippy,
  formatting and diff whitespace checks passed. Rustdoc built with Cargo's
  existing library/CLI `drip` output-filename collision warning.

## 2026-10-06: Maintainability review

- Reviewed the pending changes for unnecessary abstractions, duplicated contracts
  and code smells against the recorded DAG requirements. Local declarations,
  capability projection and discovery serve explicit requirements.
- Identified an independent schema path for handwritten kernels despite their
  associated parameter type. Smaller cleanup candidates are JSON cloning during
  typed reads, unused production reflection-documentation support, and port-role
  strings controlling formatting. Tracked these in TODO; implementation unchanged.
- Static review only; tests were not rerun. No architecture change decided.

## 2026-10-06: Typed computational/frontend boundary

- Replaced the node `view` flag with the presentation type in
  `Evaluated<Outputs, V>`. Sealed presentation implementations supply availability
  and conversion together; optional results retain their surface while empty.
  Preview, histogram and scopes now return concrete presentation values.
- Generated node constants retain their parameter struct in `TypedNode<P>`;
  graph/discovery APIs borrow its erased descriptor. The typed constructor
  derives its schema from the same parameters as the kernel. Migrated manual
  test kernels and heterogeneous declaration lists to the revised API.
- Custom controls bind through `Binding::new` with the same parameter type as
  their node. `ControlCx<P>` supplies typed reads after edits, generic schema
  controls and their graph-validated edits. Sigmoid no longer reads a raw map
  through a separate untyped settings helper.
- Independent review identified a Rust module-privacy bypass in the first
  binding layout. Moved the binding into the controls sibling module with private
  fields and read-only accessors, requiring custom node modules to use its typed
  constructor. Mathematical laws and callback intent remain trusted contracts.
- Validation: full workspace suite passed 168 tests, including eight compile-fail
  docs, with one GPU test ignored. Final GUI regression run passed all 52 tests.
  Isolated negative GUI builds rejected mismatched controls with E0308 and private
  binding literals with E0451. Strict workspace Clippy, Rustdoc, formatting and
  diff whitespace checks passed; prior platform/MSRV verification limits remain.

## 2026-10-06: Independent controls and view presentation

- Split custom parameter controls from `NodeView` body/window behavior. Local
  bindings override either independently; omitted components retain their
  schema/node-metadata defaults. Sigmoid now supplies only a controls callback,
  and preview/scopes keep the generic viewer. No built-in currently needs both.
- Added a GUI regression combining custom controls with a declared view while
  evaluation is incomplete, exercising view pop-out/open/close and retained
  inspector controls without any viewer forwarding in the binding.
- Validation: all 52 GUI tests passed (one GPU test ignored), along with strict
  GUI Clippy, formatting and diff whitespace checks. Library behavior is unchanged.

## 2026-10-06: Local presentation and typed choices

- Replaced the central node-to-widget table with node-local `view = true`
  metadata and `linkme` custom bindings beside GUI implementations. A generic
  viewer is available before evaluation; sigmoid's custom controls declare their
  own binding. Duplicate custom bindings are declaration errors.
- Added `Choice` for finite enums: explicit persisted variant names generate
  JSON serialization/deserialization and choice schemas. Migrated profile,
  intent, preview mode, scope scale, export depth/compression/level to exhaustive
  typed matches, preserving all project strings and defaults. Profile construction
  now accepts `ProfileSource` rather than an unchecked string.
- Replaced numeric parameter-kind tags with named field categories. Choice
  fields check the whole admitted schema and default at compile time, not just
  that the JSON representation is a string. Removed the completed TODO entries.
- Validation: 58 targeted core tests, 51 GUI tests (one GPU test ignored), five
  compile-fail docs and the macro declaration-rejection test passed. New coverage
  includes every built-in choice's project round trip, invalid-choice rejection,
  and a new view node's pop-out before it has any evaluated data or GUI binding.
- Independent review found no actionable issues in correctness, contained
  complexity or GUI/library separation. Strict workspace Clippy, Rustdoc,
  formatting and diff whitespace checks passed. Platform/MSRV verification
  limits from the production migration still apply.

## 2026-10-06: Remaining declaration-table audit

- Found a manual node-to-UI table in `node_ui::of`: four viewer bindings and
  sigmoid controls. Choice parameters also repeat admitted string values in
  schemas and implementation branches; wildcard branches can hide missing cases
  after a schema extension. Recorded both follow-ups in TODO.
- Node discovery and interpretation dictionaries are generated/runtime lookup
  structures, without manually enumerated members. Template node lists encode
  a particular pipeline. The closed presentation enums and renderer matches
  represent distinct rendering implementations. No processing code changed.

## 2026-10-06: Node-local documentation

- Moved every built-in node's operation help to Rustdoc on its typed function.
  The existing macro supplies `NodeKind.documentation`; optional reference links
  now live in the same `#[node(...)]` declaration and feed `NodeKind.references`.
  View/file results are described in the operation text, alongside assumptions.
- Removed the frontend's node-help table. The inspector renders node metadata
  and generated port contracts below the type ID; pop-outs remain controls/views
  only. Updated the development guideline to reflect this ownership change.
- Validation: three declaration tests and 50 GUI tests passed (one GPU test
  ignored). The inspector regression verifies the generated description's
  placement and absence from pop-outs. Strict workspace Clippy, Rustdoc,
  formatting and whitespace checks passed.

## 2026-10-06: Production semantic DAG migration

- Migrated all built-in nodes and the GUI to logical interpretations over
  `RawMat<C>` storage. Mosaics are `RealMat<1, SensorMosaic>`, RGB values are
  `RealMat<3, I>`. Removed scene/display type gates; sigmoid explicitly produces
  linear-colorimetric Rec.2020, including for later creative processing.
- Added `drip-macros`: capability parent projection through Bevy, immutable
  interpretation dictionaries, typed parameter schemas and function-derived
  node adapters/discovery. Node declarations generate their ports and linked
  catalogue entries; no manual built-in registry or type list remains.
  Shared parameter structs flatten to the existing JSON keys. Stable node IDs,
  port names and serialized parameter names remain unchanged.
- Graph edits resolve preserving chains and transactionally reject incompatible
  downstream consequences. Pending outputs remain explicit. The executor checks
  the same input requirements, named value relationships and preservation witness
  before allowing consumers/actions. Tests exercise differing runtime units and
  geometry despite identical Rust TypeIds, plus incorrect preservation provenance.
- Exposure additionally requires `ScaleInvariant: LinearRgb`: all promises of
  the input interpretation must survive positive uniform scaling. This avoids
  silently retaining future range/calibration refinements. Rec.2020 and the P3
  extension test establish the law; a bounded Rec.2020 interpretation can preview
  but cannot pass preserving exposure without explicitly weakening its contract.
- RAW and ICC resources now share explicit context-owned snapshots. Dependency
  stamps and manual invalidation semantics are retained; no filesystem watcher.
  Numerical algorithms, their notices and the wide-gamut shader path are retained.
- Claude MCP resolved `opus` to Opus 5.5. Its session denied writes/Cargo, so it
  returned a frontend patch; the primary workspace applied, reviewed and tested
  it. The editor shows resolved/pending types, names refused downstream ports,
  derives generic help from contracts and uses generated parameter tooltips.
  Existing node-specific frontend help remains optional and separate from Rustdoc.
- Validation: full workspace run passed 156 tests and four compile-fail docs are
  included in that count; its GPU test was ignored in the normal run and passed
  when separately enabled. After the final closure-law/review changes, all 55
  affected core tests and 50 GUI tests passed. Strict workspace Clippy, formatting,
  API documentation with warnings denied and diff whitespace checks passed.
- Remaining verification limits: Linux/rustc 1.96.1 only; declared added dependency
  MSRVs fit 1.92, but 1.92 and Windows/macOS execution were not run. Mathematical
  trait laws and external-data validation remain trusted; reflection is not a
  numerical proof. Earlier CLR/presentation experiments below remain unchanged.

- Independent review found sigmoid's exact nominal input unnecessarily rejected
  valid Rec.2020 refinements. Changed it to `Rec2020Rgb` capability input with a
  fresh plain Rec.2020 output and added a refinement-dropping regression. Also
  shared callback-list parsing in the macro and removed production parameter
  `Reflect` derives: parameter schemas/docs come directly from `Parameters`.
  Reflection remains on interpretation evidence; optional derive interoperability
  is exercised separately in tests. Focused re-review confirmed the issue fixed
  and found no remaining blockers; it judged the infrastructure reasonably
  contained, with shared checks and explicit trusted-law limits.

## 2026-10-06: DAG contracts from first principles

- User settled the naming split: memory layout `RealMat<CHANNELS, I>`,
  interpretation `I`, capability trait bounds. User requires mechanically
  maintained registration to be generated, eliminating separate registry
  authoring. Recorded both requirements without revisiting semantic definitions.
- Built `/tmp/drip-semantic-dag-probe` using bevy_reflect 0.19.1, linkme 0.3.37,
  syn/quote and three small macros: capability superclass exposure, interpretation
  dictionaries and node adapters/discovery. The macros total 180 formatted lines;
  this is a restricted feasibility implementation. Ordinary Rust implementations
  supply the documented laws and colorimetric operations. Existing `color.rs`
  supplies matrix calculations; no algorithms or dependencies were added to Drip.
- Fourteen tests pass: automatic dictionaries without TypeRegistry, superclass
  exposure and cross-module capability extension without central edits, rejection
  of encoded RGB despite identical storage, checking before pixels, concrete
  interpretation propagation through exposure/creative processing, matching
  preflight/runtime errors, zero-copy input storage, retained runtime matrix
  evidence, cycle rejection, generated discovery, reflected parameter docs,
  ingress shape validation, channel-count checks and parameter-type checks.
  Two temporary negative declarations fail compilation: a false capability
  exposure and an encoded interpretation passed to a linear-RGB kernel.
- The semantic probe implements only one required matrix input and one output
  explicitly preserving its interpretation. It stores `I` directly for typed
  values (unit markers are zero-sized), then retains an owned reflected witness
  across graph erasure. No manual node list, capability list or global type registry
  is required. An interpretation names its strongest capabilities; generated
  helpers recursively expose parents using Bevy adapters. This was tested with
  object-safe, nongeneric capabilities, including a capability in another module.
- Remaining work includes fixed/changed output interpretations, general layouts,
  optional/multiple ports, resource/global context integration, value constraints
  on parameters, full diagnostics/persistence/cache invalidation and equality of
  runtime logical type arguments. `MatrixRgb`'s Rust TypeId alone does not identify
  its particular basis. The toy DAG uses recursive evaluation and no caching;
  it is not a proposed replacement for production scheduling. Tests used Linux
  and rustc 1.96.1. Production code remains unchanged; documentation passes
  `git diff --check`.
- User corrected the foundational model: graph types are logical/semantic types;
  `f32` matrices are their shared storage representation, analogous to physical
  quantities sharing numeric storage. Capabilities classify the semantic types.
  Superseded the storage-first descriptor recommendation and reopened the concrete
  trait/typeclass evidence bridge. Runtime type descriptions remain possible,
  but storage identity cannot determine graph compatibility. The earlier probe
  demonstrates mechanical adapters/reflection only, not adoption of its type model.
- Investigated a concrete implementation split: typed payload descriptions,
  ordinary contract functions, Bevy parameter inspection, a narrow node macro
  and linkme discovery. Recorded the method as tentative, retaining the agreed
  `LinearRgb` semantics and unresolved-fact execution policy. Kept unrelated
  processing/platform design material unchanged.
- Built `/tmp/drip-contract-method-probe` with bevy_reflect 0.19.1, linkme 0.3.37
  and a 73-line proof-of-concept procedural macro. Twelve tests pass: generated
  discovery/port names, static parameter attributes/docs without a TypeRegistry,
  transactional parameter rejection including NaN, checking before pixels,
  shared preflight/evaluation errors, payload mismatch rejection, pending versus
  unestablished facts, creative linear-RGB interpretation, grid correspondence,
  mask bounds, generated multi-input execution and a metadata refinement rule.
  Two deliberately invalid declarations fail compilation for mismatched contract
  and kernel output types. No production code or dependencies changed.
- The new probe covers required borrowed inputs and one output, using exact
  payload casts plus instance descriptors. It does not implement the complete
  evaluator, source-result refinement lifecycle, optional/multiple outputs,
  resource invalidation, structured node/port attribution, serialization or GUI
  integration. Parameter validation is tested separately from its generated
  dispatch; an integration must expose only validated parameter snapshots.
  Macro size is feasibility evidence, not a production-infrastructure estimate.
  Validation used Linux and rustc 1.96.1; project MSRV 1.92 and other platforms
  were not tested. Test-only helpers produce dead-code warnings in the library
  build. The local crate's doc-retention feature is `reflect_documentation`.
- User defined `LinearRgb` as data intended to be linear and colorimetric.
  Recorded this as a requirement, distinct from linearity of the producing
  operation and preservation of original capture relationships.
- Audited the clarified model: creative reinterpretation is coherent, but the
  design is not yet a complete soundness specification. Exact capability laws,
  preservation/re-establishment rules, unresolved-precondition policy and
  descriptor/execution consistency still need concrete definitions and tests.
- User clarified that creative operations really are physically/mathematically
  nonlinear; their outputs are deliberately reinterpreted as new light values.
  Corrected the case study and requirements: this is an explicit node contract,
  not a weakened linearity definition, unknown fact or per-use assumption override.
- Reorganized only the DAG redesign material in DESIGN into philosophy/goals,
  case studies and requirements. Recorded pipeline freedom, conditional formal
  guarantees and bounded implementation complexity. Verified that the surrounding
  design text, including existing implementation and platform decisions, is
  unchanged by this cleanup.
- Reviewed darktable 5.6 documentation for modern calibration/grading, tone/color
  equalizers, blur/diffusion, profiled denoise, lens/perspective correction,
  retouch, masks/blending, compositing and display transforms. Consulted the
  development manual separately, without treating its additional modules as
  established 5.6 features. This is requirements research, not an algorithm port.
- User clarified that multi-frame RAW processing normally stays inside a node
  accepting multiple paths and producing one demosaiced image. Corrected the
  earlier suggestion of mandatory graph-level frame/alignment structures; track
  resources while permitting private algorithms and future typed payloads.
- Expanded the design scope after user clarification: future operations are
  open-ended, with masking, pixel math, blur and lens correction as stress tests.
  Recorded sampled-field domains, input relationships, explicit guarantee
  propagation, reusable sampling maps and separate execution footprints. Reviewed
  Lensfun's correction decomposition and OpenImageIO's alpha-association contracts
  as examples; no dependency choice or implementation follows from this review.
- Read the RAW, WB, highlights, demosaic, camera conversion, exposure, sigmoid,
  scope, export and presentation contracts as requirements for a fresh design.
  Distinguished storage, color interpretation, algorithm linearity and exposure
  meaning; identified calibration, CFA, numerical reference and grid relations
  as concrete contract candidates. Existing WB and scene/display policy is open
  for reconsideration, not silently carried into a replacement.
- Reviewed Bevy field attributes/documentation and trait adapters, clap's help
  derivation, Rust procedural macros and linkme's distributed catalogues.
  Proposed generated node plumbing with a small image-contract vocabulary;
  automatic discovery removes manual lists but does not eliminate catalogue data.
- User added consistent frontend errors as a goal. Recorded structured diagnostics
  from the same contract checks, shared across editing, loading and evaluation.
- Re-read the existing isolated Bevy probe as prior evidence. This session made
  documentation changes only; no new prototype, dependency or runtime validation.

## 2026-10-06: .NET rewrite handoff

- Populated `../drip.net/AGENTS.md` and `../drip.net/HANDOFF.md`; verified the
  instructions are byte-for-byte identical to this repository's `AGENTS.md`.
- Recorded scaffold → tests → library → GUI, F# wherever practical, Windows 11
  22H2+ and correct color on all three compositor paths. The user requires the
  DAG type system to be designed again from first principles around F#; the
  handoff deliberately does not prescribe a replacement architecture.
- Recorded `gpt-6.1-sol` with high reasoning for well-defined delegated tasks,
  with architecture and unresolved issues retained by the coordinating agent.
  No rewrite implementation started; native Windows/macOS and physical color
  validation remain outstanding.

## 2026-10-06: SDL presentation extension investigation

- Built an isolated SDL 3.4.16 Vulkan extension under
  `/tmp/drip-gui-probes/sdl-extension`: 30 added/four replaced backend lines,
  plus a 220-line application Wayland adapter and generated protocol bindings.
  No production code, dependencies or desktop settings changed.
- A pre-claim property selects FP16 passthrough from initial creation. The
  per-claim choice is immutable, support queries and creation share the same
  color-space selection, and incompatible composition changes are rejected.
  Vulkan's Wayland ownership contract and Mesa 26.2.3 source support this design:
  passthrough leaves the color-management surface to the application. Initial
  passthrough also avoids ambiguity around SDL's deferred swapchain replacement.
- C# drove two explicitly described windows for 599 frames/ten seconds, including
  main-window resize and popup hide/show. Protocol logs show two application
  descriptions with BT.709/ext-linear encoding, luminances 0/80/80, Rec.2020/D65
  target primaries and relative intent; no competing driver color owner or
  protocol error. GPU FP16 error remained 0.000410676. This checks transport and
  metadata, not physical output. Original probe against the patched library
  without opt-in also passed (603 frames), retaining its original driver metadata.
- Combined C# + Dear ImGui + patched SDL execution passed for 602 frames/ten
  seconds. Existing GPU checks for UI sRGB decoding, alpha blending over neutral
  and extended-gamut images, and negative/above-one image channels all passed.
  Its protocol log contains one explicit application description with the same
  target/luminance policy. This closes the local composition-plus-metadata probe;
  automatic ImGui Wayland viewports remain unavailable, and the two-window SDL
  test does not implement a complete ImGui multi-window UI.
- Reviewed production gaps: compositor-description failure/fallback, zero-extent
  initial claims, device-loss/reclaim and other drivers/compositors. Unsupported
  FP16/passthrough fails rather than silently changing the declared encoding;
  fallback must remove application ownership before restoring a driver-owned
  swapchain. The probe does not implement the full recovery policy.
- Windows/macOS source audit found no need to replace stock SDL's D3D12/Metal
  presenters for managed output. Windows needs explicit Advanced Color mode
  detection: ACM SDR uses display-relative white 1, HDR uses 80-nit scRGB white
  and desktop-white scaling, and unmanaged output requires LittleCMS/profile
  invalidation. Legacy DWM's 8-bit limitation is not solved by an SDL format
  extension. Metal already tags FP16 extended-linear output; future HDR must
  distinguish current from potential EDR headroom. Hardware tests remain open.

## 2026-10-06: Native GUI color probes

- User requires correct display color on Wayland, Windows and macOS. Delegated
  isolated GTK4/Gir.Core, SDL3 and Avalonia probes under `/tmp/drip-gui-probes`;
  stopped Avalonia before GUI launch at the user's request and reassigned that
  agent to Dear ImGui. No production dependencies or desktop settings changed.
- Existing Rust GPU preview/output reference test passed on Intel LNL / Mesa
  26.2.3 Vulkan. This checks shader values and FP16 storage, not physical output.
- GTK 4.22.5 / Gir.Core 0.8.1 built and ran two Wayland windows with float image
  patches and controls. FP16 compositor buffers were used, but GTK disabled color
  management because KWin did not advertise the deprecated named sRGB transfer
  function. No surface image description was sent. GTK's audited macOS output
  remains sRGB/BGRA8; Windows wide-gamut/legacy-ICC presentation was not established.
- SDL 3.4.16 / .NET 10 presented 603 frames in ten seconds. GPU FP16 blit/readback
  retained negative and above-one RGB (maximum error 0.000410676). Actual Mesa
  Wayland requests declared BT.709/ext-linear without wider target primaries;
  the protocol consequently leaves the Rec.2020 test colors outside its declared
  target volume undefined. Correct numerical transport is insufficient.
- SDL Windows/macOS source exposes the expected scRGB/extended-linear Metal
  tagging, but hardware runs remain outstanding. Windows also requires correct
  SDR-white scaling, Advanced Color state handling and legacy ICC output.
  No physical color measurements, cross-monitor tests or Windows/macOS runs were
  performed. Per-probe source, pinned upstream evidence and logs remain in `/tmp`.
- Dear ImGui docking commit `0f4b927e819823006a878bcb7fe93be129fd8d3f`
  (1.93.0 WIP) ran through a small native SDL3/SDL_GPU bridge driven by C# controls
  and state. A modified Vulkan vertex shader decodes UI RGB while preserving
  alpha coverage; image texture RGB is already linear. The final run presented
  604 frames/ten seconds; GPU readback verified opaque UI gray, partial-alpha
  blending over neutral and extended-gamut images, and negative/above-one image
  channels. This establishes composition, not final display color:
  the same SDL/Mesa target-volume omission remains. Automatic Wayland viewports
  reported unavailable; application-owned pop-outs were not implemented in this
  probe. Windows/macOS shader variants and native execution remain outstanding.

## 2026-10-06: Bevy reflection alternative

- Follow-up compiled standalone `bevy_reflect` 0.19.1 with default features
  disabled and `std` enabled under `/tmp/drip-bevy-reflect-probe`. Copied current
  image definitions, added opaque reflection/marker type paths, and reused the
  actual color module. The unrelated LibRaw metadata re-export was omitted;
  this is an isolated adapter prototype, not a complete library migration.
- Seven tests passed: connection evidence before payload creation, inherited
  capability helpers, allocation-preserving borrows/Arc recovery, scene/display
  separation, optional input/type-error distinction, absent automatic parent
  registration, generic basis-specific metadata and registry mutation failure
  (some tests cover multiple properties). Direct `#[reflect_trait]` on `RgbIn<C>`
  separately failed compilation with E0107. Both a fixed-basis bridge and a
  small generic `TypeData` projection worked; the latter avoids one bridge per
  basis. No production dependency or implementation changed.
- A deliberately mis-keyed adapter passed a metadata-presence predicate but
  returned `None` when borrowing. This is a registry-construction invariant,
  not memory unsafety or a project-file exploit. Keep typed registration helpers
  and an immutable registry; Bevy does not prove pixel semantics or discover
  arbitrary Rust trait implementations automatically. Full DAG integration,
  foreign metadata handling and performance remain untested.

- Reviewed `bevy_reflect` trait adapters, type registration, opaque derives and
  function reflection against Drip's capability descriptors and typed ports.
  Generated trait adapters address the hand-written projections without replacing
  the GUI. Explicit capability registration and graph-specific adapters remain.
- Recorded a Bevy-first feasibility proposal alongside the open CLR evaluation.
  Initial pass was documentation-only; the follow-up above supplies adapter evidence.

## 2026-10-06: CLR rewrite feasibility

- Inspected typed ports, payload storage, tiled RCD, native decoding and the
  FP16/Wayland presentation path. Researched CLR assignability, F# interop,
  native resource lifetimes, GC, SIMD, Native AOT and candidate GUI backends.
- Ran an isolated F# probe in `/tmp/drip-clr-evaluation/probe.fsx` on .NET 10.0.12:
  all 11 checks passed for inherited capabilities, scene/display and color-basis
  separation, shared object identity, typed record schemas and optional ports.
  This is a type-system feasibility probe, not a graph implementation or benchmark.
- Avalonia 12.1 supports native Wayland, but its public surface-hosting gap is
  directly relevant to independent color-managed previews. GTK4/Gir.Core exposes
  color-state-aware float textures. SDL3 exposes FP16 extended-linear output,
  but its Vulkan scRGB selection does not reproduce Drip's explicit passthrough
  declaration by itself. Recorded the migration question and prototype gates
  as open/tentative; no production code or dependencies changed.
- No GUI prototype, display measurement, numerical port or performance comparison
  was run. Cross-platform presentation and managed-buffer costs remain unverified.

## 2026-10-06: DAG type-system inspection

- Traced typed kernel tuples, capability registration, connection/load validation
  and evaluator borrowing. Rust checks kernel signatures and registration trait
  bounds; runtime graph edits check compatibility and cycles. Pixel semantics and
  custom adapter laws remain implementer obligations.
- Source inspection only; no runtime code changed or tests run.

### Runtime boundary follow-up

- Built-in exact/capability acceptance and borrowing share descriptor evidence;
  optional inputs preserve compatibility and propagate connected-source errors.
- Reproduced a project boundary bug: loading node ID `u64::MAX - 1` succeeds,
  then adding a node panics on ID increment with overflow checks enabled.
  Tracked allocator exhaustion in TODO.
- A temporary probe also confirmed that custom `EdgeValue` implementations can
  copy another type's descriptor, pass its exact-input predicate, and panic on
  borrowing. This violates the documented `Describe::<Self>` law; ordinary
  project data cannot declare such implementations.
- Validation: all 36 graph, project, typed-port and evaluation tests passed;
  both temporary probes reproduced their expected panics and were removed.
  No runtime implementation changed; this was not a decoder or numeric audit.

## 2026-10-06: unwrap/expect inspection

- Inspected production panic sites and their validation boundaries. Parameter,
  graph and typed-port reads generally rely on established internal contracts;
  public callers can still violate contracts such as `Params::validated` and
  passing an existing node to `Evaluator::run_action`.
- Preview-thread creation uses `expect` on an OS operation that can fail.
  Follow-up inspection found action dispatch also uses infallible `thread::spawn`;
  failure reaches the preview worker's panic boundary instead of an action error.
  Resource-cache lock poisoning can propagate an earlier loader panic. This
  inspection does not establish that every panic site is unreachable; no runtime
  code changed or tests run.

## 2026-10-06: editor geometry and worker handlers

- Added `CanvasLayout` as the frame's geometry snapshot, sharing node bounds and
  port positions between drawing and navigation. Extracted wire drawing/hit
  testing, individual node UI and peer menus from `Editor::show`, preserving
  widget IDs and interaction/edit ordering.
- Extracted preview evaluation/preparation and action execution from
  `worker::serve`. Command ordering, evaluator forks, thread spawning and cache
  lifetimes remain explicit in dispatch; panic boundaries are preserved.
- Renamed individual geometry to `NodeLayout`, flattened stale-result handling
  in `Worker::poll`, and centralized the worker's failure state transition.
- Validation: all 49 GUI tests passed, including editor gestures and worker
  coalescing, invalidation, export isolation and panic notification. The opt-in
  GPU test remained ignored. Clippy with warnings denied, formatting and diff
  checks passed. Live GUI behavior has not been checked.

## 2026-10-06: pointer-leave scroll handling

- Reproduced the image pop-out panic with wheel scrolling followed by
  `PointerGone`, both in one frame and on successive frames. egui retains the
  interaction position for hit-testing on the leave frame while clearing the
  hover position; residual smoothed scrolling reached an invalid `expect`.
- Image zoom now requires `hover_pos` before consuming scroll. Shared dropdown
  and numeric scrolling also requires a hover position and clears its fractional
  remainder when absent. This prevents the extra selection step reproduced on
  pointer-leave. Other Drip pointer-position users already handle absence.
- Added headless regression tests for both event timings, checking unchanged
  zoom/center and dropdown selection on leave and normal scrolling beforehand.
- Validation: all 49 GUI tests passed; the opt-in GPU test remained ignored.
  Clippy with warnings denied, formatting and diff checks passed.
- The subsequent NVIDIA/Wayland segmentation fault is not reproduced by these
  tests; panic-time native teardown remains tracked separately in TODO.

## 2026-10-06: preview interpolation parameter

- Added a saved Preview `interpolation` checkbox, off by default. Node and
  pop-out share nearest-neighbor or bilinear sampling at every zoom level.
  Sampling metadata stays separate from cached pixels and uploaded textures.
- Validation: GUI compilation and all 26 library unit tests passed. GUI tests
  passed 45 cases, including image preparation/cache reuse and pop-out navigation;
  two file-writing cases and the RAW corpus export failed on `/tmp` disk quota.
  The GPU test remained ignored; appearance has not been checked live.

## 2026-10-06: Nikon Z demo

- Added `fixtures/raw/pixls/nikon-z6ii.nef`: CC0 raw.pixls.us sample #4160,
  Nikon Z6 II (`Z 6_2` in LibRaw), 14-bit lossless NEF. Autumn woodland at
  ISO 100 with the NIKKOR Z 24–200mm f/4–6.3 VR; fine branches provide detail
  for demosaicing review. The existing PIXLS credit and LFS attributes apply.
- Included it in the corpus integration test. The corpus now has 15 images
  across 13 manufacturers, including the original Sony photograph.
- Validation: all 15 images rendered and passed the corpus checks, with zero
  skips. Clippy, formatting and diff checks passed. The new sample's as-shot
  WB and camera matrix are available; no processing changes were needed.

## 2026-10-06: usable RAW corpus

- Recorded Canon D30, Samsung GX-1L and both Sigma fp cases in TODO for broader
  RAW support and correct unpacking, retaining the upstream sample IDs. Removed
  those four files and their expected-rejection branches from the corpus test.
- The active corpus contains 14 renderable images across 13 manufacturers:
  13 CC0 PIXLS samples and Yi Cao's original Sony ILCE-7RM3 photograph.
  No decoding or processing fixes were made.
- Validation: all 14 remaining images rendered and passed the corpus checks,
  with zero skips. Clippy, formatting and diff checks passed.

## 2026-10-06: fixture integrity delegated to Git

- Removed corpus dependence on the deleted checksum manifest and verification
  script, and removed LFS-pointer checks. Test cases now list paths and expected
  decoding outcomes directly. Git/LFS handles fixture integrity.
- Validation: corpus still renders 14 images and records four metadata
  rejections; no unsupported-format skips. Clippy and formatting passed.

## 2026-10-05: cross-camera testing

- Added 17 CC0 raw.pixls.us fixtures across 14 manufacturers (about 171 MiB),
  Git LFS attributes and concise credits, including Yi Cao's original Sony
  ILCE-7RM3 photograph. Fixture integrity belongs to Git/LFS; the corpus test
  lists its cases directly, without a checksum manifest or LFS-pointer checks.

- [Corpus tests](../crates/drip/tests/pixls.rs) cover decoded metadata/CFA,
  full and 1/8 previews, finite output, TIFF dimensions and metadata, embedded
  ICC bytes, and every exported pixel against the full preview after ICC
  conversion (at most one 16-bit code difference). These are consistency
  contracts; independent algorithm references remain in the existing tests.
  `DRIP_RAW_REVIEW_DIR=/tmp/drip-pixls-review cargo test -p drip --test pixls
  --release -- --nocapture` retains sRGB TIFFs for visual review.

- LibRaw 0.22.2: 18 images tested including the original Sony; 14 rendered,
  four documented metadata rejections, zero unsupported-format skips. Canon
  D30 and Samsung GX-1L return unusable as-shot WB. Both Sigma fp DNG samples
  return zero `cam_xyz`, although ExifTool confirms nonzero ColorMatrix1/2 in
  the files. GX-1L also returns zero `cam_xyz`. No processing fixes were made.
  The binding now exposes unsupported-error classification for test skips.

- Validation: all 17 upstream checksums matched; corpus passed in about 54 s
  in release mode. The remaining workspace tests, three compile-fail doctests,
  changed-crate Clippy with warnings denied, formatting and diff checks passed.
  The existing opt-in GPU test was not run.

- Visual review covered all 14 rendered overviews and 800 × 600 native-pixel
  center crops, including the original Sony. No obvious structural corruption,
  seams or pervasive Bayer checkerboard was seen in those views. Phase One's
  violet/cool neutrals also appear in its embedded thumbnail; Pentax chart
  neutrals look slightly cyan and Samsung shaded snow blue. These observations
  do not establish color error without measured targets and illuminants.

## 2026-10-05: dropdown wheel navigation

- Shared dropdowns across parameters, Preview detail and pop-out zoom. Wheel
  navigation follows menu order and stops at either end. Reused slider scroll
  accumulation so small trackpad deltas work without scrolling the parent panel.

- Custom preview zooms appear between presets, allowing scrolling to either
  neighboring preset. Existing parameter reset menus remain available.

- Validation: 47 GUI tests passed, covering bounded dropdown navigation,
  trackpad accumulation, parent scrolling, parameter reset, project detail
  updates and custom zoom selection. Clippy with warnings denied, formatting
  and diff checks passed. The GPU-only test remains ignored; live gestures
  were not checked.

## 2026-10-05: preview pop-out navigation

- Added Fit and percentage presets, pointer-centered wheel zoom and left-drag
  panning to image pop-outs. Pixel zoom follows each window's desktop scale;
  rendered dimensions and reduction clarify the active preview detail.

- Navigation reuses the color-managed texture and stays local to the pop-out.
  Image updates retain the viewed location; zoom does not trigger processing.

- Validation: 45 GUI tests passed, including desktop-scale geometry, pointer
  anchoring, pan bounds and pop-out gestures. Clippy with warnings denied,
  formatting and diff checks passed. The GPU-only test remains ignored;
  live window interaction and moving between monitors were not checked.

## 2026-10-05: logging levels and failure context

- Separated UI status from logging. Default output covers session/file actions,
  display setup and failures; evaluation/resource diagnostics use debug, with
  cache/frame/scheduling detail at trace under module targets.

- Added typed incomplete configuration for RAW/profile/output paths. Accepted
  previews report changed root failures once, including node and generation;
  actions retain dependency causes and log destination, outcome and duration.
  Display fallback emits one warning with its protocol/capability cause.

- Validation: 134 workspace tests and three doctests passed; clippy with warnings
  denied, formatting and diff checks passed. The GPU-only test remains ignored;
  live display fallback and native window failure paths were not exercised.

## 2026-10-05: gamut-check sampling and parallel ICC conversion

- Feed Lab into LittleCMS's native gamut checker to improve sampling near black.
  Preview and export share parallel ICC conversion with the mutable pixel cache
  disabled. Softproof retains its bounded device-RGB round trip.

- On the Sony fixture at half detail (3984 × 2660, LittleCMS 2.16, 12 workers),
  release gamutcheck fell from 2.8–3.0 s to 0.68–0.69 s; softproof fell from
  3.5–3.6 s to 0.51–0.57 s. These exclude upstream processing and GUI presentation.
  Cyan coverage fell from 24.7% to 3.9%. The native mask remains approximate near
  the boundary; it does not test exact RGB coordinate bounds.

- Validation: 137 workspace tests and three doctests passed. Preview tests also
  passed with bundled LittleCMS 2.19, covering dark colors, round-trip agreement
  away from the sampled boundary and identical output across worker counts.
  Clippy with warnings denied, formatting and diff checks passed. The GPU-only
  test remains ignored; live GUI behavior was not checked.

## 2026-10-05: documentation spacing

- Added visible line breaks to node help and paragraph spacing to project docs.
  Existing wording is unchanged; only the new proofing docs were shortened.

- Validation: wording comparison, 14 GUI app tests, formatting and diff checks
  passed. Live appearance is untested.

## 2026-10-05: preview proofing

- Added normal, softproof and cyan gamut-warning modes to Preview, sharing
  profile settings and conversion with Export. The display renderer is unchanged.

- Older saved Preview nodes need the new parameters under the strict file policy.

- Validation: 130 workspace tests, three doctests, clippy and formatting passed;
  one GPU test remains ignored. Proofing tests passed with LittleCMS 2.16 and
  2.19, including agreement with bounded TIFF export. Live appearance is untested.

## 2026-10-05: categorized node selection

- Split node-kind metadata into stable type ID, internal category and default
  display name. Built-in names are capitalized; graph instances keep their
  editable names through the existing Rename/inspector path. Project files keep
  their type IDs and serialized labels, including custom names.

- Extracted the Add node selector into a small module that groups and sorts
  categories and names, returning the chosen kind to the editor. Menu styling
  now uses one inherited style function for both parent menus and submenus.

- Updated README and tracked the future dedicated picker in TODO.

- Validation: 124 workspace tests and three compile-fail doctests passed; the
  GPU-only test remained ignored. Checks include menu order/selection, renaming
  without changing type metadata, and serialized custom-name round trips.
  Workspace clippy with warnings denied, formatting and diff checks passed.
  Appearance has not been checked in the running GUI.

## 2026-10-05: parameter control gestures

- Added wheel adjustment to the shared parameter sliders, consuming panel scroll
  while hovered. Fractional wheel input accumulates in egui memory; graph values
  and schema defaults remain authoritative.

- Right-click opens Reset to default on sliders, checkboxes, dropdowns and path
  filenames, and in the existing parameter-name menu. Removed double-click reset
  after the user reported conflicts with single-click behavior. Inspector,
  pop-outs and template inputs use the same renderer. README usage documents
  these gestures.

- Slider setters reject right-button edits before egui paints, preventing the
  temporary value jump when opening Reset. A regression test reproduced the
  old mismatch and now checks displayed values on press, drag and release.

- Validation: 41 GUI tests passed, including reset menus, right-click preserving
  displayed values, wheel bounds, trackpad accumulation and panel scroll isolation. The
  GPU-only test remained ignored. Clippy with warnings denied, formatting and
  diff checks passed. The final gestures have not been checked live.

## 2026-10-05: port navigation and connection gestures

- Unified input/output port behavior: left-click opens connected-node navigation,
  left-drag pans, and right-click starts/completes a connection. Invalid targets
  preserve existing wiring; Escape/background right-click cancels the pending wire.

- Right-click disconnects the nearest hovered wire through a constant screen-space
  hit band. Nodes and controls retain input priority; wires thicken and node
  bodies gain a thin border on hover.

- Split README setup into Build & install and Usage, documenting the gesture
  rationale, navigation, connection, cancellation and disconnection behavior.

- Validation: all 36 GUI tests passed, including port navigation, connection
  cancellation/rejection, wire hit bands at three zoom levels, crossings and
  node/port input priority. The GPU-only test remained ignored. Clippy with
  warnings denied, formatting and diff checks passed. Live GUI appearance and
  gestures have not been checked.

## 2026-10-05: canvas and node gestures

- Implemented the agreed interaction table: left-drag pans over background and
  nodes, right-drag moves nodes, and right-click opens node Rename / Delete.
  Rename selects the node and focuses its inspector label field on the next pass.

- Headless interaction tests cover selection, panning/movement at three zoom
  levels, drag/menu separation, and rename/delete/add actions. All 26 GUI tests
  passed; the GPU-only test remained ignored. Clippy with warnings denied,
  formatting and diff checks passed. Live GUI gestures have not been checked.

## 2026-10-05: context menu contrast

- Added a lighter neutral background and a thin dark border to the canvas and
  parameter right-click menus through a shared theme helper.

- Validation: formatting and `cargo check -p drip-gui --offline` passed.
  Appearance has not been checked in the running GUI.

## 2026-10-05: semantic source cleanup

- Grouped demosaicing with bin2x2 and preview reduction, and scopes with shared
  settings. Grouped GUI rendering/shaders and moved reusable parameter panels
  out of the inspector. Public node exports and serialized node IDs are retained.

- Compacted project notes, linking detailed code contracts instead of repeating
  them. Kept cohesive kernels, application orchestration and worker code intact.

- Validation: 106 workspace tests, three compile-fail doctests and the explicit
  GPU color-output test passed. Clippy with warnings denied, formatting, rustdoc
  and local document links passed. The user also exercised the release GUI on
  Intel/LNL: passthrough tagging, evaluations and clean exit were confirmed.

## 2026-10-05: current presentation validation

- Shared extended-linear output and explicit Wayland color tagging implemented;
  the pipeline is documented in [display](../crates/drip-gui/src/render/display.rs).

- Prior GPU readback passed for wide-gamut images, GUI colors, scene clipping,
  out-of-volume blending and both sRGB fallback formats against independent
  colorimetry/LittleCMS references. GUI tests, clippy, formatting and rustdoc passed.

- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  over three window lifecycles. Missing extended-target support and injected
  setup failures each selected sRGB and exited normally over three runs.

- Current output remains unverified on NVIDIA, macOS and other compositors.
  Windows is best effort and untested, including HDR desktop-white handling.
  Screenshots do not establish physical accuracy; instrument verification remains.

## 2026-10-05: processing and editing baseline

- Typed graph contracts, optional metadata input, sigmoid, RCD, inpaint opposed,
  scopes and shared node controls are implemented. Detailed behavior, source
  deviations and reference vectors live beside the implementations/tests.

- Earlier workspace validation covered synthetic/reference algorithms, real RAW
  export, graph/cache lifetimes, compile-fail contracts and headless GUI behavior.
  Exiftool/exiv2 readback matched the ARW capture metadata; capture time was checked
  under UTC, Tokyo and Los Angeles. Broader photo/camera validation remains.

- Canvas/pop-out interaction was exercised by the user and Wayland parenting
  tested live. Hidden-window pacing and native dialogs were not retested after
  their fixes. macOS/Windows setup and packaging remain unverified locally.

## Performance evidence

Historical timings explain choices; they are not current latency guarantees.
On the Sony fixture, Ryzen 5600G/12 workers and RTX 3080, the 2026-10-04 trial
measured Rayon at 162 ms versus CubeCL CPU 582 ms/GPU 623 ms, with a retained RAW
pyramid. Removing that pyramid reduced decoded-resource storage from about
226 MB to 85 MB, excluding node results and temporaries.

The 2026-10-05 RCD/highlight pipeline measured release level-change medians of
1.20 s at 1/2 and 2.64 s at Full over seven cycles with 12 workers. These include
sensor processing/cache replacement, exclude GUI preparation/upload/draw, and
have uncontrolled filesystem caches. Exposure/sigmoid edits reuse upstream
results. Reproduce with [preview_latency](../crates/drip/examples/preview_latency.rs)
and measure whole frames before tuning.
