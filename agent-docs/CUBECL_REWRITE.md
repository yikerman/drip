# CubeCL rewrite

Date: 2026-10-08. Active implementation record for local branch
`cubecl-rewrite`. Earlier design studies are proposals and evidence, not
specifications. User statements remain separate in
`TYPE_SYSTEM_REDESIGN_USER_NOTES.md`. Original DESIGN/PROGRESS/TODO logs are
unchanged as requested. No commits or pushes have been made.

## Decisions

| Status | Decision |
| --- | --- |
| Requirement | Rewrite `drip-*` directly. Core and macro first, frontend adaptation second, optimization afterwards. |
| Requirement | Preserve the general GUI design, universal node controls/views, popups and interaction logic. Internal APIs and architecture-dependent docs/contracts may change. |
| Requirement | Fixed algorithms with concrete payloads, independent outputs and host descriptions. Evaluator owns CPU/device movement. No persistent graph image-result cache. |
| Decided | `DRIP_BACKEND` selects WGPU (default), CPU or CUDA in CLI computation, GUI previews and GUI exports. CPU/CUDA require their Cargo features; invalid or unavailable selections report errors. Rendering stays on wgpu. |
| Decided | CubeCL 0.11 computation; existing wgpu presentation and color management remain frontend-owned. Initially transfer view endpoints through host memory. |
| Decided | Ordinary Rust contract functions and typed read/write parameters; macro generates handles, discovery and invocation. No constraint DSL or algorithm inference. |
| Decided | Transactional edits check nominal payloads, cycles and all known descriptor relationships. Templates may connect before source binding; later bindings recheck the whole graph. Missing data fails only demanded evaluation. |
| Decided | One topological run shares fan-out and retires intermediates after the final consumer. Batch status requests retain inputs only for observers. Independent failed branches do not block good branches. |
| Decided | Source loading produces an immutable value or stored loading error, outside computation. Failed RAW paths remain editable/persistable; rebind or invalidate reloads snapshots. |
| Decided | RAW separately outputs Bayer, BayerGains, CameraMatrix, CaptureMetadata and BayerLevels. Missing calibration does not invalidate usable mosaic output. |
| Decided | Version 2 stores named ports, serde parameters and opaque frontend layout. Headless target loading instantiates only target ancestry, excluding unrelated GUI node kinds. |
| Requirement | A global context goes to every node and contract. Demosaic internally reduces its Bayer phase planes according to that request; there are no temporary/hidden DAG nodes. Full detail/export use scale 1. |
| Requirement | Nodes are ordinary operations with DAG bindings, not the finest algorithm stages. Shared graph machinery belongs in DAG/eval; private preparation stays in the operation. |
| Decided | Sigmoid is one node; coefficient preparation and curve sampling use private helpers. See `NODE_BOUNDARY_AUDIT.md` for the broader boundary audit. |
| Requirement | Independent Astra/high review with minimal author rationale; examine correctness, regressions and removable infrastructure. |
| Open | Full-resolution RCD still uses full-image scratch. Tiling, shared-device presentation, concurrent scheduling and CUDA/graphics interop remain follow-up work. |

## Implementation

`drip` owns payloads, contracts, graph edits, evaluation, computation, profiles
and persistence. `drip-gui` owns its pure observer declarations, scopes,
proof/texture preparation, file actions, interaction state and worker mailbox.
The editor's adapter stores labels/layout/template flags; parameters and edges
have one source in the core DAG. Both frontends reuse the same starting pipeline.

The RAW path includes site gains, independently balanced clipping levels,
opposed reconstruction, RCD, camera matrix, exposure and sigmoid. RCD and sigmoid
use the same CubeCL kernels on enabled runtimes. Bayer reduction is one shared
CubeCL kernel used by both the explicit reduction node and demosaic's internal
processing. Opposed and explicit RGB reductions are CPU operations; placement
is declared and evaluator-owned transfers bridge them. CPU storage is packed RGB; device RGB has a padding lane.

Macro parsing/validation, adapter generation, documentation and derives are
separate modules. Optional read ports are explicit. Node Rustdoc supplies help;
declared payload labels are generated for typed ports. Parameter schemas drive
universal controls, and `#[gui_node]` supplies frontend-only custom bindings.

GUI behavior tests were restored after the unauthorized replacement GUI.
Canvas gestures, universal controls, per-node popups, help placement, template
inputs, detail selection, proofing, scopes and export remain. RAW binding and
project opening run on the worker. Latest binding tickets and preview generations
reject stale completions. Current view images survive while a new preview runs.

## Preview correction

The first adapted preview reduced only RGB endpoints, removing the detail
control's effect on RCD computation/memory. An attempted correction inserted
hidden reduction nodes in a temporary graph. The user clarified that the global
context was intended for this purpose: demosaic itself owns its scale-dependent
processing. Both graph rewriting and its fallback policy are removed.

`GlobalContext { scale }` is passed per evaluation. `KernelContext.global`
exposes the same value to computation, while generated contracts receive it
explicitly. Connect-time descriptions use full detail. During evaluation, each
node's contract checks its actual upstream descriptions and global context
before output allocation. Returned descriptions therefore match actual storage;
context-dependent incompatibilities fail at evaluation without changing the DAG.
Demosaic averages CFA phase planes into local device scratch, then runs its same
interpolation algorithm. Upstream gains/highlight processing remain unscaled.

Inspected darktable's pinned `src/iop/demosaic.c` at
`61dea294bedb3ab6c7cca1a45530b1ab5c0461f3` and current upstream demosaic source.
Its full path requests sensor-scale input and can resize after demosaic;
a separate approximate path directly constructs reduced RGB instead of running
full demosaic. That is not generic resizing of the alternating CFA values.
Drip uses CFA-plane averaging plus the selected demosaic instead of adopting
that algorithm switch. This is an approximate preview, not an exact reduction
of full-resolution nonlinear processing.

RCD's input/output/scratch footprint is approximately 72 bytes per input pixel,
excluding allocator overhead. Reducing each dimension by two divides this
portion by four. Sensor correction and source retention still use full Bayer
resolution. The Full/export path still needs the larger working set.

## Independent review

Astra/high reviewed code and diffs without author rationale, including the GUI
boundary. It found:

- Missing RAW assets prevented project repair. Fixed with immutable binding-error
  snapshots, preserved parameters and source-rooted evaluation errors.
- Public reduction nodes lacked factor controls. Added parameter schemas.
- An unavailable calibration output could poison a usable sibling mosaic in a
  batch. Omitted undescribed outputs now fail their consumers only; a producer
  omitting a promised output still fails.
- Repeated panel serialization was incidental overhead. Schema panels now read
  one snapshot; graph name lookup uses frontend state directly.
- KernelContext exposed a full CubeCL client despite stronger documentation.
  Documentation now states the transfer convention accurately.

The reviewer found the Node/Port/Payload/Run infrastructure proportional and did
not recommend another abstraction layer. Follow-up findings in the now-removed
preview graph planner are superseded by the global-context correction. The final
context/contract/evaluation review found misleading preview detail labels and
finite Bayer-average overflow. Labels now state requested detail; the averaging
kernel uses bounded accumulation. The node-boundary follow-up also removed
duplicate highlight output allocation and repeated sigmoid plot preparation.
See `NODE_BOUNDARY_AUDIT.md` for retained boundaries and the metadata tradeoff.

## Validation

The previous integrated workspace/GPU suite, restored GUI behavior tests,
renderer reference, both frontend builds and RAW-to-TIFF smoke passed. These
preceded the global-context correction.

After the global-context correction and node-boundary audit, the full workspace
suite passes on the real GPU, including all 74 GUI tests, algorithm reference
checks, RAW preview detail changes and 13 macro tests. CPU-backend tests also
pass. New cases cover unchanged node/edge topology, context-specific output
allocation, downstream contract rejection, full-detail restoration, finite
extreme Bayer averages, and agreement with explicit Bayer reduction followed by
full-detail RCD. Macro tests verify context forwarding.

Validation commands use `CC=/usr/bin/cc CXX=/usr/bin/c++` and
`CARGO_TARGET_DIR=target/cubecl-rewrite`:

- `cargo test --workspace --offline -- --include-ignored` (real GPU)
- `cargo test -p drip --features cpu --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings` (passed)

The sandbox GPU adapter could not allocate the large RAW buffers; the same
suite completed successfully on the real device.

Tests cover:

- nominal/interpretation mismatches, transactional rollback, cycles, foreign and
  stale handles, deferred source binding and descriptor-changing edits;
- multi-output sharing, optional inputs, failure isolation, no persistent image
  caching, producer storage checks and CPU/GPU transport;
- upstream RCD/opposed vectors, all Bayer phases, odd/small extents, sigmoid
  extremes, CFA-plane reduction and CPU/GPU kernel agreement;
- restored GUI interactions, Sony RAW detail changes, stale completions, async
  binding, image lifetimes, independent TIFF/proof parity and renderer reference;
- project roundtrip, missing-asset repair, explicit source rebind and headless
  loading of a GUI project's selected dependency subgraph.

## Remaining work

| Work | Status |
| --- | --- |
| Core/macro and original GUI adaptation | Implemented; global-context and node-boundary reviews complete, CPU/GPU tests pass. |
| Legacy project migration | Version 1 rejected explicitly; automatic conversion not implemented. |
| Preview scaling | Demosaic consumes global scale internally; no hidden nodes. Full/export request scale 1. |
| Full-resolution bounded-memory RCD | Open; current scratch is proportional to image extent. |
| Shared compute/render device and CUDA interop | Deferred. Host presentation endpoint is explicit. |
| Cross-platform runtime/package validation | Unverified beyond this Linux environment. |
| Performance optimization | Deferred by user instruction. |

## Benchmark archive

Measurements, plots/LaTeX, prepared pixels, retired PoCs and benchmark build caches
are outside this repository at:
`/home/yikerman/Developer/drip-benchmark-results/2026-10-08/`.
Their former relative paths are retained under `agent-docs/`,
`experiments/runtime-bench/` and `experiments-source/runtime-bench/`.
No measurements were deleted or rerun during the rewrite. CUDA counter profiling
was stopped by the user after `ERR_NVGPUCTRPERM`; no driver settings were changed.


## Runtime selection follow-up

`RuntimeContext::from_env()` owns the common environment selection. Frontends
use it for every evaluator, including the GUI's separate export task. Direct
library constructors remain explicit and ignore the environment. CPU feature
activation alone does not change the default or silently fall back.
GUI configuration errors use the existing per-node failure/status path, so an
invalid backend completes the request with an error instead of leaving the
worker busy.

Validation passed:

- `DRIP_BACKEND=cpu cargo test --workspace --features drip/cpu --offline`,
  including 74 GUI tests; GPU-only tests remain ignored in this CPU run.
- CPU CLI export of the Nikon D70s fixture to `/tmp/drip-backend-cpu.tiff`.
- Default-feature selector test rejects CPU without its feature. CLI subprocess
  checks reject an unknown value and unavailable CUDA before loading input.
- Strict workspace/all-target Clippy with `drip/cpu`, formatting and diff checks.

The setting selects computation explicitly; it does not automatically retry a
failed GPU run on CPU.


## RAW port labels

**Fixed:** the handwritten RAW adapter used `type_name` fallback labels, which
expand aliases to generic Rust types. Generated nodes preserve the declared
payload aliases. RAW now declares the same payload labels on its outputs;
connection checks still use payload identity and interpretations.


## Node-declared display labels

**Decided:** `#[node(..., port_labels(image = "RGB", output = "RGB"))]`
overrides displayed payload labels using the port argument names. Unspecified
labels retain their existing defaults. Unknown port names, duplicate entries
and non-string labels fail macro expansion. Handles, persisted endpoint names,
payload identity and interpretation contracts are unchanged.

The handwritten RAW adapter continues to declare labels directly; this option
applies to function-based node declarations and does not add asset-binding
infrastructure to the macro. All 15 macro tests and compiled graph adapter
tests pass. The source-organization follow-up rechecks the whole workspace.


## Sigmoid helper consolidation

**Fixed:** coefficient preparation is a private function in `sigmoid.rs`.
Removed `sigmoid_prepare.rs`, its module boundary and the intermediate struct
used only to pack the 22 kernel constants. Curve mathematics and upstream
notices are retained; THIRD_PARTY now points to the combined source file.


## Node ownership and source layout

**Requirement:** each node owns a module; its parameters, contract and private
kernels stay together. All image-domain behavior in `drip` belongs under
`src/node/`. Files outside it contain shared DAG/evaluation infrastructure.
GUI node-specific code belongs under `drip-gui/src/node_ui/`.

**Implemented:** RCD has `node/rcd/{mod,kernel}.rs`; sigmoid and small-node kernels
stay in their own node files. Shared matrix, CFA reduction and RGB reduction
operations live in `node/shared_kernel.rs`. Colorimetry, profiles, TIFF encoding,
concrete image/calibration data and templates moved under `node/`. Generic
`Payload`/`Interpretation` interfaces and `F32Buffer` remain infrastructure;
node discovery/adapters live in `definition.rs`.

GUI observer declarations now accompany their node bindings. Scope settings,
histogram/waveform reductions and plotting remain shared under `node_ui/scopes`;
chromaticity math belongs to `node_ui/vectorscope.rs`. Preview preparation data
and the default pipeline layout also moved under `node_ui`. Node IDs, ports,
math and GUI interaction rules are preserved. Rust module paths changed;
frontends, tests and generated adapters were updated together.

The concise rule is in `crates/drip/README.md`. Earlier source paths in this
record describe their historical state. Workspace/all-target compilation,
CPU workspace tests, GPU workspace tests (including ignored tests), and strict
workspace/all-target Clippy with the CPU feature pass. Formatting and whitespace
checks pass.

## Inferred node parameter schemas

**Decided:** `#[node]` obtains `Parameters::SPECS` from the function's second
argument type. Removed the `parameters = ...` option and repeated declarations
across processing and GUI nodes. Settings structs implement `Parameters`,
normally through the derive; `()` supplies an empty schema. Handwritten node
adapters still supply their metadata explicitly.

The compiled adapter regression checks a derived boolean schema and an empty
unit schema; macro parsing rejects the removed override option.
All workspace test targets compile. The 15 macro tests and 16 host model tests
pass (the GPU test remains ignored); strict workspace/all-target Clippy and
formatting checks pass.

## Passive node timing

**Requirement:** worker trace logging reports per-node timings without adding
device waits or changing scheduling. The proposed per-node synchronization was
rejected before implementation.

**Decided:** evaluation optionally records host elapsed time for each attempted
node, including input materialization, contracts, dispatch and release. Shared
ancestors appear once; upstream-blocked nodes have no timing. Existing download
waits are attributed to the consumer; the terminal runtime wait remains outside
node timings. This is not GPU kernel duration. GUI preparation is timed as a
separate phase. Records are emitted after processing, outside the preview total;
clock reads and record storage still have a small cost. Debug keeps total-only
logging, while trace enables the breakdown.

**Validated:** CPU workspace tests and the WGPU hybrid regression pass. Timed
hybrid evaluation preserves the output; shared ancestors and failure skipping
retain their behavior. The GUI logging regression checks both phases and trace
enable/disable behavior. Strict workspace/all-target Clippy and formatting pass.
