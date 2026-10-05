# Drip design notes

Every decision has a status:

- **requirement**: the user's goal. Changing it needs the user's agreement.
- **decided**: agreed with the user. On 2026-10-04 the user approved the session-1 design as a whole, with the answers recorded per entry.
- **tentative**: proposed by the engineer and not yet agreed.
- **open**: no proposal yet, or a proposal is waiting for the user.

Each entry gives its rationale. Superseded entries are struck through and stay in place so the history remains readable.

## 1. Structure and platforms

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| S1 | Cargo workspace with three crates: `drip` (library), `drip-cli` (binary `drip`), `drip-gui` (binary `drip-gui`). | requirement | All processing, the graph, color management and persistence go in the library. Frontends stay thin, and the library never depends on a GUI. |
| S2 | Linux on Wayland is primary. Windows and macOS must build in CI but are tested only loosely. X11 is unsupported. | requirement | |
| S3 | License: AGPL-3.0-or-later. | decided | The user chose AGPL. It is compatible with LibRaw (LGPL-2.1 or CDDL-1.0) and LittleCMS (MIT). Still to confirm: `-or-later` or `-only`. |
| S4 | CI runs fmt, clippy and tests on Linux, and builds on Windows and macOS. | decided | From the handoff. Native dependencies (LibRaw, LittleCMS) will need per-OS install steps once they're added (see L3). |

## 2. Libraries

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| L1 | Raw decoding uses LibRaw only. Color management uses LittleCMS through the `lcms2` crate only. | requirement | |
| L2 | Write our own minimal LibRaw binding: a small C shim compiled with `cc` against the LibRaw headers, plus one safe Rust function `decode(path) -> Result<RawData>` that copies everything we need into owned Rust values and closes the LibRaw handle before it returns. | decided | Survey of existing bindings, October 2026: **rsraw 0.1.1** is unsound, because `RawImage::open(&[u8])` hands the buffer to `libraw_open_buffer`, which keeps the pointer, but `RawImage` has no lifetime tied to the buffer (use-after-free). It also leaks the handle when open fails, has no MSVC support, needs bindgen/libclang at build time, and vendors LibRaw 0.21.3. **libraw-rs-sys** vendors LibRaw 0.20.1 and was last touched in 2021. **libraw/libraw-sys** date from 2015. Putting a C shim in front of LibRaw means Rust never mirrors LibRaw's struct layout, which changes between releases, and keeps the unsafe surface to a handful of functions. No LibRaw handle ever escapes, so thread safety only requires linking the reentrant `libraw_r`. |
| L3 | Linking: system `libraw_r` through pkg-config on Linux (Fedora ships 0.22.2 and needs `LibRaw-devel`), Homebrew on macOS, vcpkg on Windows. Vendoring the LibRaw source is the fallback if those prove unreliable in CI. | decided | The system library gets security fixes from the distribution. Vendoring makes builds reproducible, but then we own LibRaw's build. |
| L4 | TIFF is written with the pure-Rust `tiff` crate, and the ICC profile goes in tag 34675. | decided | The requirements don't name a TIFF library. Still to check: the encoder must write arbitrary tags and 16-bit and float samples. |
| L5 | `lcms2` transforms are created per export and confined to that call. No native handle (LibRaw or LittleCMS) ever lives inside a `Value`. | decided | Values are `Send + Sync` by construction. Thread safety of `lcms2` `Transform` depends on its context type, and is checked against the pinned crate version when M2 starts. |

## 3. Processing model

### 3.1 Values and port types

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| P1 | Port types are **semantic** and drawn from a closed runtime enum, `PortType`. Values form a matching enum, `Value`, whose payloads are immutable and held in an `Arc`, so values are cheap to clone and `Send + Sync`. | decided | Connections are type-checked at runtime: the GUI checks a connection as it's made, and the loader checks a whole file. That needs types as runtime values that can be compared and serialized. A closed enum keeps matches exhaustive and serialization trivial, and adding a type is a local change. An open `TypeId`/`dyn Any` scheme would allow plugins but loses both of those properties, and nothing calls for plugins. |
| P2 | Prototype types: `Mosaic` (1-channel f32 CFA data with its CFA pattern), `CameraRgb`, `SceneRec2020` (scene-referred linear Rec.2020), `DisplayRec2020` (display-referred linear Rec.2020, nominally 0 to 1), `RawMetadata`. The three RGB types are 3-channel interleaved f32. Every image carries its geometry relative to the sensor (scale and origin), so later ROI work and spatial parameters have a coordinate system. | decided | Keeping scene-referred and display-referred Rec.2020 as separate types makes "tone map before the output transform" a rule the type system enforces: Export accepts only `DisplayRec2020`. |
| P3 | **Colorimetric characterization travels with the image.** `Mosaic` and `CameraRgb` carry an `Arc<CameraInfo>`: the XYZ→camera matrix (LibRaw's confusingly named `cam_xyz`) and the as-shot multipliers. WB and camera→Rec.2020 read it from their image input. The raw reader still has a `RawMetadata` output (exposure, lens, timestamps) for informational nodes and future EXIF passthrough. | decided | If the matrix and multipliers travelled on a separate metadata wire, a user could wire one file's metadata into another file's image and get silently wrong colors, which type checking can't catch. This refines the handoff idea rather than contradicting it, since the reader still outputs both. |
| P6 | White-balance state is **not** tracked in the types for now. A graph that applies WB twice still type-checks. | decided | The user's call (2026-10-04). It keeps types simple and allows debayer-then-WB pipelines. |
| P4 | An input port accepts a *set* of types. An output port has exactly one type. A connection is valid when the output's type is in the input's set. An input takes at most one edge. An output may feed any number of inputs. | decided | This covers Histogram accepting any image type, with no subtyping machinery. Preview accepts only `SceneRec2020` and `DisplayRec2020`, since it has no display interpretation for a mosaic or camera RGB. Viewers attach as branches. Inserting one inline would need a pass-through output, which they deliberately don't have. |
| P5 | Bayer only in the prototype. The raw reader rejects X-Trans, Foveon and linear DNG with an error. The CFA pattern is represented generally (w×h color indices) so X-Trans can be added later. | decided | 2×2 binning has no meaning for X-Trans. The check sits at the input boundary, which is the only place checks belong. |

### 3.2 Nodes

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| N1 | Separate the **node kind** from the **node instance**. A kind is a static descriptor in a registry: stable name (e.g. `raw.read`), default label, parameter schema, static input and output ports, `eval`, actions. An instance holds `&'static NodeKind` plus data: label, params, external parameters, opaque `ui`. The registry is needed only to resolve kind names when loading files and to list kinds for frontends. | decided | Behavior lives in one place per kind. Holding the kind directly means graph and evaluator APIs need no registry, and a graph can't refer to a kind that doesn't exist. Ports are static. Variadic nodes would need ports computed from params, which nothing needs yet. |
| N2 | Parameters are declared by a schema (`name`, kind: float with range / int / bool / enum / path / ICC profile / …, default, flags) and stored as a map from name to `ParamValue`. `eval` receives them through a typed accessor. | decided | One declaration serves the GUI (widgets), persistence (defaults and tolerance), the CLI (`--set node.param=value`) and templates (the per-image flag, see F4). |
| N3 | `eval(params, inputs: &[Value], ctx: &EvalContext) -> Result<Evaluated>`, where `Evaluated = { outputs: Vec<Value>, view: Option<View> }` and `View` is its own enum (`Histogram`, `Image`), separate from port values. Eval must be deterministic and free of side effects. | decided | Determinism is what makes the caching in E2 sound. `view` is explained in U1. |

### 3.3 Evaluation

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| E1 | **Pull-based.** A frontend asks for a set of target nodes. The evaluator topologically sorts their ancestors and evaluates only those. | decided | The GUI requests every graph node regardless of canvas position (G11). Export requests only its dependencies; unrelated branches cost nothing for that action. |
| E2 | Dependency stamps hash node kind, canonical parameters, the global level and each input connection with its upstream stamp. One result is cached per node. | decided | Files remain cached until the evaluator is replaced. Per-file reload and revision tracking were removed in E19; paths still participate through parameters. Source identity prevents rewiring from reusing errors naming an old upstream node. |
| E3 | Errors are values, kept per node. A failing node records its error, its descendants are marked blocked, and nodes in other branches still evaluate. | decided | The GUI can show the error on the node itself while the rest of the graph keeps working. |
| E4 | `EvalContext` carries a downscale level `k`, which gives `scale() = 2^k`. Storing the level makes non-power-of-two scales unrepresentable. Later it will also carry a region of interest and a cancellation token. | decided | This leaves room for the deferred items: 1:1 viewing of the visible region (ROI), background evaluation (cancellation) and GPU work. None of them is built now. |
| E5 | **Preview scale is applied in the raw reader**: after black subtraction, same-color CFA sites are averaged with the CFA phase preserved, and odd edges are cropped, so the reduced image is still a valid Bayer mosaic. Spatial parameters are expressed in full-resolution pixels and divided by `ctx.scale` inside nodes. | decided | Interactive editing must work on a reduced image, but debayering needs a real mosaic. Binning inside the CFA satisfies both. Expressing parameters at full resolution keeps preview and export consistent. |
| E8 | Dev builds use `opt-level = 1`. | decided (user, 2026-10-04) | At opt-level 0 the real-raw tests took about 105 s, against 11 s at opt-level 1. Image processing has to be usable in development. |
| E6 | The interactive session keeps one node-result cache at preview scale. Full-resolution export has no persistent node-result cache: within one run each node is evaluated once, and its outputs are held until their last consumer has run. Resources are shared with preview (E17, E19). | decided | A 45 MP 3-channel f32 image takes about 540 MB, so caching every node at full resolution isn't viable. Resource retention is separate from intermediate image lifetimes. |
| ~~E7~~ | ~~LibRaw's decode (scale-independent and expensive) is memoized in a session resource cache keyed by path. It is not node state.~~ | superseded by E11 (user, 2026-10-04) | The normalized pyramid replaces the decoded u16 buffer, also removing repeated normalization and binning. |
| E9 | Prioritize interactive responsiveness, cross-platform support and ease of kernel development over peak throughput. Maintain exactly one source implementation of each computational kernel across CPU and GPU backends. | requirement (user, 2026-10-04) | Preview-level transitions currently interrupt interaction. Backend-specific compilation and dispatch may differ; handwritten copies of the algorithm are excluded. |
| ~~E10~~ | ~~Use CubeCL 0.11.0-pre.4 for the five computational nodes, selecting hardware GPU or native CPU at runtime.~~ | superseded by E15 (user, 2026-10-04) | The implementation was evaluated, then replaced with Rayon for simpler development and better measured host-to-host latency. See 3.3.2–3.3.3. |
| ~~E11~~ | ~~Retain a normalized f32 RAW pyramid across levels and projects.~~ | superseded by E17 and E19 (user, 2026-10-04) | The earlier level-switch benchmarks used this policy. Manual levels no longer justify retaining every normalized level. |
| E12 | Run interactive evaluation and CPU texture preparation on a persistent frontend worker. Retain the last completed preview and coalesce pending node-target requests at the global level. | decided (user, 2026-10-04); implemented | The existing synchronous library evaluator and its one-result-per-node cache stay on the worker. Monotonic request generations reject stale results across edits and project replacement; completion wakes the window loop. See 3.3.6. |
| E13 | Acceleration is an implementation detail of selected nodes. Preserve existing host-value inputs/outputs, node contracts, evaluator, graph, caches and renderer; ordinary nodes may coexist with accelerated ones. Maintain one kernel source across its execution backends (E9). | requirement (user, 2026-10-04) | The user requires changes to stay inside some nodes and remain transparent to other infrastructure. Backend selection, runtime reuse and temporary buffers belong behind the node implementation. Include allocation, transfers, synchronization and output materialization in performance comparisons. Cross-node device residency, graph fusion and renderer integration are outside this change. |
| E14 | Preview latency below 300 ms is acceptable; prioritize straightforward development over further throughput tuning. Linux is the first target; Windows/macOS are lower priority. | requirement (user, 2026-10-04) | A future worker can tolerate longer computations while keeping the UI responsive. Initial RAW loading and full-resolution processing are measured separately. |

| E15 | Use Rayon for white balance, 2×2 debayering, camera-to-Rec.2020, sigmoid and histogram. Remove CubeCL and postpone GPU computation. | decided (user, 2026-10-04); implemented | Ordinary Rust parallel iterators use one algorithm with one or multiple CPU workers. No JIT, compute device, runtime buffers or host transfers; graph and node contracts remain unchanged. Measured latency is lower than CubeCL on the test machine (3.3.3). |

| E16 | Preview level is one manually selected global setting; exports always evaluate at level 0. | requirement (user, 2026-10-04); implemented | The GUI offers Full through 1/256, defaults to 1/2 and persists the level in project UI state. All targets share one level through the unchanged evaluator API; one cached result per node remains sufficient. |
| E17 | Cache decoded RAW data and derive only the requested normalized mosaic. Keep ordinary dependency-stamp caching. | requirement (user, 2026-10-04); implemented | Normalization and repeated same-phase downsampling reuse the existing algorithms, discarding temporary levels. Only the current result per node remains cached. Preview and export share decoded data until project replacement or invalidation (E19); see 3.3.7 for the measured cost. |

| E18 | Use the existing lowercase status style: “evaluating…” for pending preview work, “running export…” for export, then “done” or the error. | requirement (user, 2026-10-04); implemented | Keep the previous preview available while a new result is computed. Current failures replace the old view with an error. |

| E19 | Share one resource store between preview and exports; replace it on cache invalidation or project replacement. Remove per-file reload and revision tracking. | decided (user, 2026-10-04); implemented | Cloning the store shares both loaded values and future first loads. Existing exports keep their store; a new project cannot retain the previous project's resources. No path enumeration or revision hashing is needed. |

| E20 | An action consumes its evaluator; interactive callers fork once before dispatch. | decided (user, 2026-10-04); implemented | The export worker already owns an isolated evaluator. Removing the second fork avoids duplicate setup, retains full-detail evaluation and releases its temporary node cache when the action ends. A caller preserving preview state uses `evaluator.fork().run_action(...)`. |

### 3.3.1 Kernel survey (2026-10-04)

This initial survey informed the CubeCL trial (E10), subsequently replaced with Rayon (E15). Measurements follow in 3.3.2–3.3.3.

| Approach | One kernel source? | Execution and development cost |
|----------|--------------------|--------------------------------|
| Rust functions with Rayon | Yes, for CPU execution | Serial iteration or parallel iteration calls the same function; a Rayon pool can have one or multiple workers [5]. Small change to the current code, ordinary debugging and no GPU runtime. It does not provide GPU compilation. |
| CubeCL | Yes, CPU and GPU | `#[cube]` functions compile through an intermediate representation to backend code. GPU targets include Vulkan/SPIR-V, Metal and WebGPU through wgpu [2]. CPU uses LLVM-based compilation and workers [3], [4]. The v0.11.0-pre.4 thread pool dispatches one task per cube unit; a one-unit launch selects one kernel worker [8]. This is a restricted Rust DSL, not arbitrary Rust/Vec/iterator code. The project describes its API as alpha; pin and test a version rather than assuming development-branch documentation matches the release. |
| rust-gpu, optionally with krnl | Shared Rust arithmetic is possible | Compile GPU-compatible Rust to SPIR-V and call shared functions from CPU loops. GPU entry points, buffers and scheduling still need integration; reductions and synchronization are harder to share than pixel functions. rust-gpu requires a particular nightly toolchain [6]. krnl's example shares the arithmetic but supplies separate CPU iteration and GPU dispatch [7]. More integration work than the CubeCL runtime approach. |
| Handwritten WGSL with wgpu | GPU source only | Portable GPU execution, but a separately maintained Rust CPU kernel violates E9. A software GPU adapter is not a substitute for a straightforward native CPU backend. |
| OpenCL from Rust | A shared OpenCL kernel, not a Rust kernel | Supports CPU/GPU devices where implementations exist, but Rust host bindings do not turn Rust functions into OpenCL kernels. Apple deprecated OpenCL in macOS 10.14 and recommends Metal [9], making it a poor fit for the platform priority. |

Vulkan and OpenCL are execution APIs; SPIR-V is an intermediate representation, not a competing runtime [10]. For Drip, the source language and runtime should be selected together. Under E13, accelerated nodes consume and return existing CPU values. Transfers are part of their cost; a candidate must save enough computation to outweigh that cost. Cross-node device residency is not a prerequisite or part of this change. Headless processing remains in the library, independent of the GUI.

The trial covered pixel maps, CFA processing and histogram reduction. Tests use independent expected values, without a second production implementation.

### 3.3.2 CubeCL trial (2026-10-04, superseded)

CubeCL 0.11.0-pre.4 was integrated into five nodes with one kernel source and
host-valued boundaries. CPU and wgpu/WGSL backends passed the numerical tests.
A wrapper handled backend selection, dispatch, transfers and readback. The CPU
backend brought a JIT compiler and statically linked LLVM; the stripped Linux
benchmark was 105.3 MiB. Linux CPU and RTX 3080/Vulkan were tested;
Windows/macOS packaging and other GPU vendors were not verified.

The trial demonstrated that this architecture works, but the Rayon comparison
below showed lower complete-call latency with much less code. CubeCL, its
wrapper and its dependencies have been removed, along with the experiments
folder. Historical measurements remain in PROGRESS; no alternate production
kernels are retained. A future GPU proposal must still satisfy E9 and E13.

### 3.3.3 Rayon inside processing nodes (2026-10-04)

**Decided:** the five computational nodes call ordinary Rust kernels in
`crates/drip/src/nodes/kernels.rs`. Indexed parallel iterators produce their
output Vec directly. Histogram chunks accumulate private counters in place,
then reduce them with exact integer addition; comparing bin edges avoids log
rounding at boundaries. Chunking also avoids copying the 3 KiB accumulator
through a per-pixel fold. Sigmoid retains the stable logarithmic formula so
extreme finite inputs do not overflow intermediate powers.

Rayon's shared pool executes the same implementation with one or multiple
workers [5], [15]. There is no CPU/GPU dispatch wrapper, JIT, unsafe buffer
transport or duplicate serial implementation. The production kernel/dispatch
code shrinks from about 390 lines to about 105 (excluding tests). Nodes still
consume and return existing host values. RAW preparation, evaluation, caches,
GUI rendering and export scheduling are unchanged. The processing library no
longer depends on CubeCL or wgpu; GUI wgpu remains. Workspace MSRV returns to
1.92.

Release measurements on the 7968×5320 Sony fixture, Ryzen 5 5600G (12 logical
CPUs), RTX 3080/Vulkan. The same `preview_latency` benchmark warms shapes and
cycles levels 3/2/1/0 seven times. All columns include complete graph evaluation
of preview and histogram, allocations and replacement of previous values,
including transfers for CubeCL. They exclude GUI texture preparation, upload
and drawing. These runs retain the same RAW pyramid cache for a fair comparison.

| Preview level | Output RGB pixels | Rayon, 1 worker | Rayon, 12 workers | CubeCL CPU | CubeCL GPU |
|---------------|-------------------|-----------------|-------------------|------------|------------|
| 3 | 165,336 | 60.7 ms | 53.2 ms | 51.2 ms | 53.3 ms |
| 2 | 662,340 | 55.4 ms | 9.0 ms | 24.2 ms | 28.0 ms |
| 1 | 2,649,360 | 244.2 ms | 36.6 ms | 129.7 ms | 123.3 ms |
| 0 | 10,597,440 | 1,037.7 ms | 161.9 ms | 582.3 ms | 623.3 ms |

These are medians, not latency guarantees. Level 3 follows level 0 and includes
releasing much larger previous values, explaining its higher time than level 2.
The Rayon 12-worker level-0 range was 159.5–177.9 ms. Initial level-3 evaluation,
including RAW decode/pyramid construction and pool startup, was 458 ms (Rayon),
619 ms (CubeCL CPU), and 1,067 ms (CubeCL GPU). Filesystem and driver/JIT caches
were uncontrolled; these are not cold-disk measurements. The stripped Rayon
benchmark is 0.84 MiB; these executable sizes are not GUI installer sizes.

Verification covers all Bayer phases and odd crops, empty images, concurrent
calls, uneven lengths, matrix values against f64 references, 25 sigmoid parameter
combinations and extreme finite inputs (2e-6 absolute tolerance), and every
histogram edge plus adjacent f32 values (exact counts). Workspace tests also
check the real RAW pipeline against LibRaw and TIFF output. Kernel/node tests
run with one worker as well as the default pool.

```sh
RAYON_NUM_THREADS=12 cargo run --release -p drip --example preview_latency -- fixtures/raw/sony-ilce-7rm3.arw
RAYON_NUM_THREADS=1 cargo run --release -p drip --example preview_latency -- fixtures/raw/sony-ilce-7rm3.arw
# Add DRIP_BENCH_TRACE=1 for individual node wall times.
```

GPU computation is postponed. Preview-level policy, cache simplification and
background evaluation will be planned separately after this implementation;
none is changed by the Rayon replacement.

### 3.3.4 Preview level and background evaluation plan (2026-10-04)

**Original implementation plan, written after the Rayon commit.** All four steps
are implemented; resource retention now follows E17 and E19.
The resulting worker behavior is recorded in 3.3.6. The original visibility-based
target policy below was subsequently removed by G11.

**1. Manual global preview level — implemented.** The GUI selector offers
levels 0–8 (Full through 1/256), default 1 (1/2), saved in `Project::ui` and
validated when loading frontend state. Automatic adaptation and image-size
feedback have been removed. Zooming or resizing a view only changes its
presentation. The GUI calls the existing `evaluate(graph, level, targets)`;
`EvalContext` passes the level to source nodes. `run_action` continues forcing
level 0 independently. With the current 2×2 debayer, level 0 still produces
half the sensor dimensions (C6). The cache and worker changes were kept separate
from the detail-level implementation.

**2. Simplify RAW retention — implemented (3.3.7).** Remove the normalized `Pyramid` and derive only
the requested mosaic from decoded data. Keep one current result per node: that
cache prevents recomputation on every frame and unrelated edit. Proposed resource
policy (now implemented): retain decoded RAW data for the active project, share
it with exports, and release it on project replacement or cache invalidation
once existing readers finish. This preserves the I/O saving without retaining all f32 levels or all
previous projects. E19 replaces the original revision/snapshot proposal with a
shared resource store; existing exports keep their store after invalidation.
Manual level changes will rescan the decoded sensor; benchmark this separately
from the Rayon numbers, which used the unchanged pyramid cache. This scope
removes the added level cache, not every cache in the application.

**3. Move preview evaluation to one persistent worker.** Keep `Evaluator`
synchronous and GUI-independent. Add a small worker adapter in `drip-gui` with
these ownership boundaries:

| Owner | State and work |
|-------|----------------|
| UI thread | Editable project, global level, visible target set, latest completed presentation, pending request |
| Preview worker | Evaluator, resource revisions, node-result cache, RAW loading, processing and presentation preparation |
| Existing export worker | Immutable graph/resource snapshot, full-resolution evaluation, encoding and file writes; at most one export |
| Renderer | Texture upload, GPU objects and drawing |

Every evaluation request names its target node(s). The graph snapshot supplies
their definitions and connections; the level specifies how to evaluate them.
For example, requesting a preview targets that preview node, requesting a
histogram targets that histogram node, and export names an export node plus its
action. A batch of preview/histogram targets evaluates the union of their
ancestors once, sharing upstream results. An empty target set does no work.

The worker request therefore carries `(graph, targets, level, epoch, generation)`;
its presentation results are keyed by `NodeId`. Coalescing replaces the complete
latest target set, not just the last individually requested node. Export is a
separate `(graph, node_id, action)` command whose upstream evaluation uses level
0. Neither a graph nor a resolution alone identifies an evaluation request.

The frontend submits these requests; preview, histogram and export nodes never
invoke the evaluator. Preserve the existing visible-node target policy, including
ordinary processing nodes so their errors remain visible. The library still
walks only target ancestors. Changing which nodes count as targets is a separate
product decision, unnecessary for the worker.

Submit a graph snapshot, level, targets, project epoch and request generation
only when evaluation-relevant state changes. Keep one preview request in flight;
while it runs the UI replaces one pending request with the latest state. Avoid
cloning or comparing the whole graph every frame: edits report changes, while
changes to the visible target set are tracked separately. A level or parameter
edit supersedes old work; moving a node without changing visibility does not.

The worker sends completion tagged with its epoch/generation and wakes the
window loop through a winit user event (3.3.5).
The UI accepts only the current generation, retains the previous completed view
while busy, and immediately dispatches the newest pending request when the
worker finishes. Opening/New increments the epoch so reused node IDs cannot
accept results from the previous project. A failed current evaluation publishes
its errors; an old successful image must not silently masquerade as current.
No mutex around a live evaluator and no per-frame blocking receive.

Reload, project reset and export are ordered commands, never coalesced preview
requests. Before launching an export, the preview worker snapshots resources
after earlier reload commands and uses the graph captured at the click. Later
edits/reloads cannot alter that export. Keep the existing separate export thread
so encoding does not queue all new previews behind it. Both paths use the same
Rayon pool; tune its worker count only if simultaneous export hurts UI scheduling.

Initially let an in-progress node finish and discard obsolete presentation.
Cooperative cancellation between nodes is a later, small evaluator extension
if measured stale-work latency warrants it; LibRaw cannot be interrupted safely
in the middle of its current call. Handle worker exit/panic as a visible error,
and shut down through channel closure without joining long computation on a
UI frame.

**4. Complete the CPU/UI boundary.** Evaluation alone is insufficient:
`preview::upload` currently packs every RGB f32 pixel into RGBA f16 on the UI
thread. Move that conversion into a GUI-side preparation stage on the worker,
reusing prepared pixels when the source Arc is unchanged. Publish views/errors
and prepared texture bytes rather than all intermediate node outputs. Leave
wgpu upload and drawing in the renderer. Retire replaced large CPU buffers on
the worker after render callbacks release them; the measured level-0→3 cost
shows that destruction itself can stall interaction. Bound retained generations
to the displayed snapshot, active computation and latest pending request.

Validation should cover saved level round-trips and invalid input, unchanged
level while zooming/resizing, full-resolution export at every preview setting,
RAW reload/export snapshot lifetime, rapid edits with latest-result selection,
project replacement with reused node IDs, bounded pending work, worker failure,
and export/reload ordering. Use a deliberately slow test node to prove UI
callbacks return while computation is blocked elsewhere. Finally measure frame
time during RAW loading, edits and export, including texture upload and buffer
retirement; the compute-only benchmark cannot establish UI responsiveness.

### 3.3.5 Worker investigation after manual detail (2026-10-04)

**Investigation preceding the implementation in 3.3.6.** The existing synchronous
`Evaluator::evaluate(graph, level, targets)` can remain intact. A temporary
external probe successfully kept an evaluator on a persistent standard thread,
sent graph snapshots and global levels over channels, and evaluated preview and
histogram targets at levels 3/2/1/0. No async runtime or node API changes were
needed. The production adapter must carry explicit target IDs as in 3.3.4;
there is still one global preview level and one result per node.

The code investigation identifies these integration points:

| Module | Required change |
|--------|-----------------|
| `drip-gui::worker` (new) | Own evaluator and preparation cache; receive explicit node-target requests, reload/reset/export commands; emit generation-tagged presentation snapshots and completion/errors. Keep the adapter in the frontend. |
| `App`, `inspector`, `editor` | Replace direct evaluator reads with the last completed snapshot. Report successful parameter/topology edits, aggregate them after drawing, then submit the latest complete target set and global level. Labels, selection and template-input flags do not affect computation; geometry matters only when visibility changes. |
| `main` | Use a winit user event to wake the sleeping window loop when a result or worker termination is reported. |
| `preview`, `views`, editor layout | Consume prepared images and per-node view/error information; avoid sending intermediate output images to the UI. Keep texture upload and draw calls in the renderer. |
| Export dispatch | Ask the evaluator-owning worker for the graph/resource snapshot after earlier reloads; keep the existing separate export thread. |

`main.rs` currently sleeps in `ControlFlow::Wait` or waits for a frame deadline.
It neither installs egui's repaint callback nor receives user events. Calling
`Context::request_repaint` from a worker therefore does not by itself wake this
integration [16]. Prefer a small `WorkerReady` event sent through
`EventLoopProxy` after enqueueing the completion [17]; `Shell::user_event`
requests a redraw, and the frame drains results without blocking. Pass a wake
callback into the adapter so tests do not need a window. Signal termination on
both normal exit and panic, otherwise a waiting UI can stay busy indefinitely.
This avoids polling and does not require a general repaint-callback rewrite.

`editor::layout` reads only a node's view/error, not its output values. A compact
presentation snapshot is therefore sufficient. Returning whole `NodeResult`s
would retain unnecessary large images on the UI thread. The worker must retain
ownership needed to free replaced image buffers off-thread; simply wrapping
results in Arc does not determine where their last reference is dropped.

The GUI currently converts RGB f32 to RGBA f16 inside `preview::upload` on the UI
thread. An external release probe copied that exact conversion, including Vec
allocation, and measured seven repetitions per image. Ryzen 5600G, Sony fixture,
Rayon with 12 workers for evaluation:

| Global level | RGB pixels | CPU texture preparation, median |
|--------------|------------|---------------------------------|
| 3 (1/8) | 165,336 | 1.3 ms |
| 2 (1/4) | 662,340 | 6.3 ms |
| 1 (1/2) | 2,649,360 | 21.5 ms |
| 0 (Full) | 10,597,440 | 118.4 ms |

These measurements exclude driver upload, drawing and buffer destruction; they
are not measured frame times. They confirm that moving evaluation alone would
leave a substantial full-detail CPU stall. Prepare the same texture bytes on the
worker before publishing a view, reusing them for unchanged source images. No
new conversion algorithm or second compute kernel is needed.

Recommended next implementation: one frontend worker, one in-flight preview
request plus one replaceable pending request, reliable ordered control commands,
and explicit completion wake-up. Retain the library API and existing global-level
cache. Test the scheduler with a blocked synthetic node, including completion
while the window is idle, target-set replacement, project changes with reused IDs,
reload/export ordering, and worker termination. Integrate CPU texture preparation
and buffer retirement before claiming that computation is entirely off the UI.
RAW pyramid removal remains an independent pending change; it is not necessary
to establish this worker boundary. Do not add cancellation inside kernels or a
new async runtime in the initial implementation.

### 3.3.6 Implemented preview worker (2026-10-04)

**Decided and implemented.** `drip-gui::worker` owns a persistent evaluator and
its resource/node caches. The library, node contracts and
`evaluate(graph, level, targets)` API are unchanged. GUI requests carry an
immutable graph snapshot, explicit target IDs, the global detail level, and a
monotonically increasing generation. Generations never reset when a project
reuses node IDs; a separate project epoch is unnecessary.

The UI sends at most one preview request at a time and retains only the latest
pending request. Each GUI request targets every graph node. Parameter/topology
edits, detail changes and cache invalidation request work; canvas geometry,
labels and selection do not themselves invalidate image
computation. The current evaluation finishes normally, then obsolete completion
is discarded and the latest pending request runs. There is no cancellation yet.
The last completed view remains available while the status line says
“evaluating…”. A current failure publishes the node error instead of keeping an
old image as if it were current.

Reset, cache invalidation and export commands use the same ordered channel. Export snapshots
the graph at the click and shares the current resource store when the command is processed,
after preceding invalidations. The existing separate export thread then runs the full-
detail action by consuming that evaluator without another fork (E20), then reports
completion. The GUI still permits one export at a
time; preview and export computation can run concurrently. Both reuse the same
Rayon pool. Project replacement starts with a fresh evaluator and resource store
(E19). RAW resources retain decoded data rather than a normalized pyramid (E17).

“Invalidate cache” replaces the worker's entire evaluator with a fresh instance,
discarding every node result (including errors) and loaded resource. It advances the request generation and discards
pending work so an earlier completion cannot become current. All graph nodes
are requested again. Last presentations remain for stable layout and display
until replaced; they cannot supply evaluator results. Existing export snapshots
remain valid, while subsequent exports use the fresh resource store.

The worker publishes per-node views/errors, not intermediate output values.
Each accepted completion replaces the presentation snapshot. There is no
viewport intersection test, visible-target tracking or merging of off-screen
presentations. Deleted nodes and project resets release their presentations.
Images are converted to RGBA f16 before publication. A worker-owned preparation
cache reuses bytes by source Arc identity and keeps a reference to each image
until renderer/UI references are released. After a frame drops its previous
callbacks and textures, a collect command frees unreferenced prepared images
and their source images on the worker. Wgpu upload, drawing and GPU object
lifetimes remain on the render thread.

Completion is enqueued before a winit `WorkerReady` user event requests a redraw.
Polling receivers never blocks the UI, and no periodic completion timer or async
runtime is needed. Worker panics also notify the window and end the busy state;
export panics become action errors. Closing the app sends shutdown without
joining a potentially long RAW decode or kernel on the UI thread.

Validation covers an explicitly blocked node while the UI displays “evaluating…”
and opens a control, preserving the previous image until completion; latest-only
pending requests and complete target sets; cache reuse; project reset with reused
node IDs; ordered reload/export and full-resolution action inputs; worker panic
notification; exact prepared texels and off-thread allocation ownership. Existing
real RAW, TIFF and global-detail tests continue to pass. Driver upload/draw latency
still needs end-to-end measurement; asynchronous CPU work alone does not promise
a particular frame-time bound.

### 3.3.7 RAW retention cleanup (2026-10-04)

**Decided and implemented.** The resource store caches decoded `Raw` values.
Each raw-node evaluation normalizes the sensor, repeatedly downsamples to the
requested global level and drops temporary levels. No pyramid is retained.
Unchanged raw nodes still reuse their ordinary evaluator result. A project
replacement or invalidation releases the resource store after existing exports
finish. Changing paths within one project retains earlier reads until that reset.

On the same Sony fixture and machine as 3.3.3, seven release repetitions of the
preview/histogram level cycle gave the following medians. These include node
allocations and cache replacement, excluding GUI preparation/upload/drawing;
filesystem caches were not controlled. The level-3 transition follows level 0
and includes retiring its larger intermediates.

| Level | Before: retained pyramid | After: decoded RAW |
|-------|--------------------------|--------------------|
| 3 (1/8) | 55.0 ms | 282.1 ms |
| 2 (1/4) | 12.0 ms | 219.4 ms |
| 1 (1/2, default) | 45.0 ms | 244.1 ms |
| 0 (Full) | 189.0 ms | 358.0 ms |

Initial decode plus evaluation was 499.8/485.9 ms before/after. The decoded u16
resource is about 85 MB instead of the roughly 226 MB f32 pyramid, excluding
current node results and temporary full-size normalization. Full-detail changes
exceed the earlier 300 ms preference; computation remains off the UI thread.
Tests compare LibRaw output, independent CFA averages, regenerated levels and
export results; a deleted source file proves subsequent levels/forks reuse the
decoded data, and a weak reference verifies old normalized levels are released.

### 3.4 UI-only nodes and side effects

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| U1 | UI-only nodes are ordinary nodes with zero output ports. What they compute for presentation (histogram bins, the preview image) goes into `Evaluated::view`, which any node may set. The library computes views. Frontends read and draw them; the GUI draws each view inside its node (G7). | decided | Nothing is special-cased. A histogram in the middle of a pipeline is just another target the GUI asks for. Views are plain data, so they can be tested headlessly. |
| U2 | Side effects are **actions**. A node kind may declare named actions, each with its own evaluation policy; Export declares `export` with policy full scale. A frontend calls `run_action(node, "export")`. The library then evaluates that node's inputs against a snapshot of the graph and resources and runs the action with those inputs, params and an evaluation context. `eval` itself never writes files. The CLI runs `export` on every node that declares it. Still to define: what happens on overwrite, on destination collisions between exports, and when some exports fail. | decided | This meets "side effects on explicit action, not on every re-evaluation" and gives export its full-resolution evaluation. The alternative, treating export as a sink that writes during eval, fails three ways: the cache would skip a repeated export, the preview scale would leak into the export, and a stray GUI request could write files. |


## 4. Color

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| C1 | Prototype pipeline: raw.read → white_balance → debayer.bin2x2 → camera_to_rec2020 → tonemap.sigmoid → export.tiff, with preview and histogram as UI-only nodes. | requirement | |
| C2 | Normalization in raw.read works on the untouched `raw_image` after `unpack`. LibRaw's `subtract_black` is not called. Per site, black = `black + cblack[c] + cblack[6..]` pattern at the site's (row, col) in sensor coordinates. Every site is divided by one common denominator, `maximum − black`, as dcraw does. Values below black are kept negative, not clamped. Then crop to the visible area. Verified against LibRaw 0.22 source: `unpack()` doesn't call `adjust_bl()`, so the raw fields hold. The denominator's common black is what `adjust_bl` derives. Patterns of at most 2×2 are first folded into the per-color offsets, so it becomes the smallest black of a Bayer cell. Otherwise it is `black + min(cblack[0..4]) + min(pattern)`. The second green keeps its own as-shot multiplier when the camera records one. The black pattern is indexed by visible-area coordinates. | decided | LibRaw's black level model has several layers. Its `adjust_bl` folds the common black into `cblack`, so mixing processing stages subtracts black twice. A per-channel denominator would act as a hidden white balance. Keeping values below black avoids biasing the noise floor. |
| C3 | Camera→Rec.2020 follows dcraw [1]. With `A` = XYZ→camera (`cam_xyz`) and `B` = Rec.2020→XYZ (D65), `T = inverse(diag(1 / (A·B·1)) · A · B)`. The as-shot multipliers, normalized to green = 1, are applied first, by the WB node. Prototype is 3-color Bayer only. If the multipliers or the matrix are missing or invalid, raw.read fails. | decided | Going straight to Rec.2020 uses one matrix and doesn't depend on LibRaw's sRGB-targeted `rgb_cam`. Row normalization maps white-balanced camera neutral to D65 white. That is still an adaptation model (von Kries in camera space), so "no chromatic adaptation" means "no separate CAT step such as Bradford". The user confirmed this reading. |
| C4 | Tone mapping: per channel, `y = x^c / (x^c + k)` on exposed scene values `x = max(0, v · 2^exposure)` (Michaelis–Menten/Naka–Rushton form), with `k = g^c (1/g − 1)` for g = 0.18 so middle grey maps to itself. Parameters: exposure (EV, default 0), contrast c (default 1.5). It maps [0, ∞) onto [0, 1). | decided (user, 2026-10-04) | It's the simplest curve with a fixed grey point, a contrast control and a soft shoulder. Per-channel application desaturates and shifts hue in bright saturated areas. The user asked to keep it this simple for the prototype, with no color adaptation (no hue preservation or gamut mapping). A better tone mapper is in TODO. |
| C5 | Export goes `DisplayRec2020` → LittleCMS transform → output profile (built-in, C7, or a user ICC file) → TIFF, with the profile embedded. The source profile is built in memory as Rec.2020 primaries, D65 and a linear TRC. Accepted output profiles: RGB color space, class display, output or color space, checked when the user picks the file. Parameters: rendering intent, black-point compensation, sample format (`u16` or `f32`), compression (none, or deflate with a level). The `tiff` crate offers deflate levels fast (1), balanced (6) and best (9). u16 output is clipped to [0, 1] explicitly before encoding. | decided (user, 2026-10-04) | Taken literally, "arbitrary ICC profile" covers gray, CMYK, device-link and abstract profiles, which aren't interchangeable RGB destinations. Whether LittleCMS float transforms stay unbounded depends on the profile: tabulated TRCs and LUT profiles clamp. Perceptual intent does nothing for matrix/shaper profiles. A float TIFF with an arbitrary ICC profile is unusual, and readers handle it inconsistently. |
| C6 | 2×2 binning debayer halves each dimension. A "full-resolution" export therefore evaluates the full sensor data but writes a half-size TIFF. | requirement consequence | Stated explicitly so it isn't mistaken for a bug. |
| C7 | Built-in output profiles: `srgb`, `display_p3`, `rec2020`. They are matrix/shaper profiles on D65, with the sRGB curve (sRGB, Display P3) or BT.2020's inverse OETF (Rec.2020) as ICC parametric curves, and carry a description. | decided (user, 2026-10-04) | Common outputs shouldn't need a profile file. |

### 4.1 Display path

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| D1 | The library produces linear Rec.2020 views. Turning them into display output is the frontend's job, using library helpers (LittleCMS) where an app-side transform is needed. | decided | This keeps the library independent of any GUI. |
| D2 | **The compositor does the display transform.** The GUI presents an scRGB (`ExtendedSrgbLinear`, `Rgba16Float`) swapchain, which the Vulkan WSI tags for the compositor. Rec.2020 content is converted into it with one 3×3 matrix and scaled so that SDR reference white is 203/80 = 2.5375. Drip has no app-side display transform for now. | decided (user, 2026-10-04) | Spike on the dev machine (KWin 6.7.5, NVIDIA 615): the WSI sends `set_primaries_named(srgb)`, `set_tf_named(ext_linear)`, `set_luminances(0, 80, 203)`. The user asked about simply declaring the surface as Rec.2020. That works on KWin, but only if Drip tags the `wl_surface` itself through a second Wayland connection on winit's display and relies on the WSI leaving sRGB swapchains untagged. That behavior is undocumented, and wgpu deliberately drops `BT2020_LINEAR`/`PASS_THROUGH`. scRGB carries Rec.2020 exactly (negative values) and is also the native path on Windows and macOS. Either way, egui's sRGB UI and the images need converting into one space, so declaring Rec.2020 saves nothing. |
| D3 | App-side display transform (compositors without color management): deferred, see TODO. | decided | It follows from D2. |
| D5 | Display verification, 2026-10-04: six synthetic Rec.2020 patches, captured from KWin screenshots (8-bit sRGB). scRGB with the 203/80 scale gave the same pixels as tagging the surface `bt2020`/`ext_linear`, and matched the plain sRGB route within rounding for in-gamut colors. | decided | Screenshots clip to sRGB, so correct display beyond sRGB still needs a colorimeter measurement. The user has one; the measurement is postponed (TODO). |
| D6 | If the surface offers no scRGB, the GUI falls back to an sRGB swapchain. Previews are then clipped to sRGB, and a warning is logged and shown in the window. | decided (user, 2026-10-04) | The app keeps working on compositors or drivers without wide-gamut support, and the user knows. |
| D4 | GUI toolkit: winit + wgpu + egui (`egui-winit` and `egui-wgpu`, not eframe). | decided (user, 2026-10-04) | See section 6. |

## 5. Persistence

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| F1 | Format: JSON through serde (`serde_json`). It is pretty-printed, which costs nothing extra in code and diffs better. | decided (user, 2026-10-04) | It is universal, scriptable (`jq`) and can be edited by hand. |
| F2 | File layout: `{ format: "drip", version, nodes: [{id, label, kind, params, bindings, ui}], edges: [{from: [id, port], to: [id, port]}], arguments, ui }`. Node IDs are stable `u64`s that are never reused while a graph is open. `nodes` is a list, not a map keyed by ID, so a duplicate ID is an error instead of being silently collapsed by the JSON parser. Labels are unique and non-empty. Ports are referenced by name. `ui` fields are opaque JSON owned by the frontend (node positions, view state). Relative paths will resolve against the project file's directory (M2). | decided | These are the handoff's ideas. Giving the frontend an opaque `ui` blob lets the GUI persist its layout in the library's file format without the library knowing anything about the GUI. |
| ~~F3~~ | ~~Per-kind versions and migrations, opaque unknown nodes, kept unknown params.~~ | superseded by F6 (user, 2026-10-04) | |
| F6 | **No compatibility in the prototype.** Files carry the crate's major version (`version`) as a global tag, and a different one is rejected. Beyond that, a file must match exactly what this build writes: unknown fields, unknown kinds or parameters and missing parameters are all load errors. There is no migration and no default-filling. | decided (user, 2026-10-04) | Compatibility code is complexity the prototype doesn't need. Exact matching makes any mismatch loud instead of silently losing data (e.g. older files' bindings). |
| F7 | **External parameters.** Any parameter can be *external*: a template input that takes a value per image. The schema gives the default (`raw.read.path` and `export.tiff.path` are external), and users can change it per node. Every node has its own inputs, so a second raw reader brings its own `path`. A project is the graph with those values filled in; `template()` resets them to their defaults. Values live in the nodes' ordinary params, so there are no bindings or argument maps. Nodes are identified by their stable numeric id; labels are unique, renamable names for people and the CLI. | decided (user, 2026-10-04) | The inputs of a template are computable from the template itself. Sharing one input between nodes isn't needed, and it was the source of binding-compatibility rules. Dropping arguments also removed the evaluator's dependency on `Project`. |
| ~~F4~~ | ~~Per-image params flagged in the schema, bound by node label.~~ | superseded by F5 | |
| ~~F5~~ | ~~Parameters bound to named graph inputs; projects supply arguments.~~ | superseded by F7 (user, 2026-10-04) | |

## 6. GUI toolkit (open)

Evidence from the dev machine (Fedora 44, KWin 6.7.5, NVIDIA 615.71, GTK 4.22.5):

- **KWin**: `wp_color_manager_v1` v2, parametric only. It has named primaries including bt2020, named transfer functions `gamma22`, `ext_linear`, `bt1886` and `st2084_pq`, and the `windows_scrgb` feature.
- **NVIDIA Vulkan WSI on Wayland** offers `R16G16B16A16_SFLOAT` swapchains with `BT2020_LINEAR_EXT` and `EXTENDED_SRGB_LINEAR_EXT` color spaces, so an app that owns its surface can present linear wide-gamut content directly.
- **GTK 4.22**: compositor color management is opt-in (`GDK_DEBUG=color-mgmt`). In a spike that showed a Rec.2020-linear `GdkMemoryTexture`, GTK bound KWin's `wp_color_manager_v1` and destroyed it straight away, even with the flag set, and never tagged the surface. Not yet diagnosed (probably a feature or version mismatch). GTK owns the `wl_surface`, so Drip can't tag it itself.
- **wgpu 30** exposes `SurfaceColorSpace::ExtendedSrgbLinear` (scRGB), which can carry Rec.2020 through out-of-range values. It has no Rec.2020-linear variant.

Decided: **winit + wgpu + egui** (`egui-winit` and `egui-wgpu` directly, not eframe, so Drip configures the surface). egui renders into an ordinary sRGB offscreen texture. A final Drip pass composites the decoded UI and the Rec.2020 image views in linear light and writes an scRGB float swapchain tagged for the compositor. egui's renderer needs no patches. Node-editor widgets exist for egui (e.g. egui-snarl). It ports easily to Windows and macOS.

Caveat: blending *inside* egui stays in gamma space. "Composites everything in linear Rec.2020" holds for the UI and image layers, not for egui's own translucent widgets.

Details from Codex's review that the plan must cover: convert Rec.2020 into linear-sRGB coordinates while keeping negative and above-one values. Choose a reference-white scale, since Windows-scRGB defines 1.0 as 80 cd/m² and SDR white is commonly 203 cd/m². egui's offscreen target is a gamma-encoded UNORM texture with premultiplied alpha, so it has to be unpremultiplied, decoded and premultiplied again when composited. The driver's WSI owns surface tagging when it presents in scRGB, so Drip must not attach its own color-management surface as well.

Spike done (protocol level, see D2). Still to do in M3: verify what is actually displayed, with saturated and neutral patches, alpha edges and a numeric check of the presented values. egui renders into an offscreen gamma-encoded `Rgba16Float` target, where image views are drawn sign-preserving and extended-sRGB-encoded, so z-order stays egui's. A final pass decodes into the scRGB swapchain.

### 6.1 GUI design (M3)

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| ~~G1~~ | ~~Custom node editor in a bottom panel, with a separate preview panel and histogram in the sidebar.~~ | superseded by G7 (user, 2026-10-04) | |
| ~~G2~~ | ~~Evaluate visible nodes synchronously, adapting resolution to drawn image size; export on a background thread.~~ | superseded by G8 (user, 2026-10-04) | Automatic detail changes interrupted interaction. |
| ~~G8~~ | ~~Evaluate visible targets synchronously at a manual global level; export separately at level 0.~~ | superseded by G9 (user, 2026-10-04) | Detail selection remains; evaluation moves off the UI. |
| G9 | Evaluate node targets on a persistent worker at the global preview level. Render the last completed presentation and show lowercase activity messages. Export retains its separate full-detail action thread. | decided (user, 2026-10-04); implemented | Image computation, texture-byte preparation and CPU image retirement are off-thread; the renderer owns GPU upload/drawing. Target selection follows G11. See E12, E18 and 3.3.6. |
| G10 | “Invalidate cache” clears all evaluator node results and loaded resources, then recomputes all graph nodes. | decided (user, 2026-10-04); implemented | Replaces the former per-file reload action. Includes nodes without file dependencies and every loaded resource; preserves the last presentation during recomputation and existing export snapshots. |
| G11 | Remove visibility-based evaluation. The GUI requests every graph node at the global detail level. | decided (user, 2026-10-04); implemented | Canvas position, pan, zoom and resizing no longer select targets or schedule evaluation. Off-screen results and errors stay current; ordinary dependency caching and node-targeted export remain unchanged. |
| G12 | The histogram's range is set per node by integer `min_ev` (−24 to −1, default −12) and `max_ev` (+1 to +10, default +4), and its plot by `scale`: `linear` or `log` (default, `ln(1 + n)`, replacing the former square root). The plot marks and labels 0 EV; its labels scale with the canvas. | decided (user, 2026-10-04); implemented | Ranges on either side of 0 keep the bounds ordered and 1.0 always on the plot without a cross-parameter check. The default template feeds the histogram display-referred values, which the sigmoid keeps below 1.0, so nothing falls above 0 EV there. |
| G3 | Logging uses the `log` facade in the library and `env_logger` in the frontends. Frontends also show warnings and errors in the window. | decided (user, 2026-10-04) | The standard, minimal choice. |
| G4 | Undo/redo is postponed. | decided (user, 2026-10-04) | Not needed for the prototype (TODO). |
| G5 | One minimal UI style in `theme.rs`: everything on middle grey (sRGB 118, 18% linear), dark text, no shadows, rounding or borders. Fills distinguish elements; color is reserved for errors and histogram channels. | decided (user, 2026-10-04) | A neutral surround is standard for judging color. Decoration distracts from the image. |
| G6 | The GUI is tested headlessly with `egui_kittest`: opening, previewing, starting a new project, error reporting, the gamut warning and the inspector. Rendering is verified by the screenshot comparison (D5). | decided | Exercises the app's wiring without a GPU. |
| G7 | **The node editor is the main canvas.** It pans (drag) and zooms (wheel, around the pointer, up to 1 per G13). Preview and histogram nodes draw their view inside their body (images fitted, histograms plotted) and are resizable from a corner. Positions and sizes are saved in each node's `ui`. Opening a project centers the whole graph and zooms out until it fits, never in past 1. Any number of preview or statistics nodes can be shown. The sidebar holds the selected node's parameters (right-click a name to make it a template input or fix it) and the template's inputs. | decided (user, 2026-10-04) | Each node hosting its own presentation removes "which preview to show" logic and scales to many views. Enlarging a preview into its own window is planned in G13. |
| G13 | **Each node kind has a frontend GUI, and nodes can pop out into their own windows.** A window shows one part of one node. Parameters are the same for every node: the editor's frame shows a ⚙ button in the header of nodes that have any, and their window is the inspector's view of the node. Everything else is the kind's: a `NodeGui`, looked up by kind with an empty default, sets the node's body size, draws the body on the canvas, and can pop any part of it out into a window it names and draws. Preview and histogram nodes draw their view with a 🗗 button over its corner that pops out the view. It works through a node context that exposes the node's result, edits to its own parameters, `ui` and label through the graph's validated operations, popping out and requesting actions. Shared widgets (parameters, views, pop-out button, resize corner) keep kinds small. The editor keeps the frame: header, ports, error row, wires, selection and dragging. The canvas becomes one egui layer transformed for pan and zoom (`set_transform_layer`), so bodies hold real widgets; zoom is capped at 1. A pop-out is a separate OS window titled `<label> <part> · Drip`, with its own surface, egui context and renderer on the shared device; closing it restores the node's body on the canvas. Pop-outs are children of the main window, so the window manager keeps them above it and minimizes them with it. Windows redraw together only when the graph, results or pop-outs change. Which nodes are popped out is not saved, and there is no docking: the window manager arranges and pins the windows. All windows present with `Fifo`. | decided (user, 2026-10-05); implemented | Nodes decide their own GUI, so `App` manages less and knows no node kinds; parameters are the one behavior every node shares, so the frame owns them. A single per-node window chosen by kind could not pop out a histogram's parameters. Window layout is left to the window manager instead of reimplemented. Spikes, 2026-10-05: widgets on the transformed layer take input correctly at zoom 0.5–1, popups anchor to them unscaled, and previews land in place once the paint callback uses the transformed rect; the layer needs the inverse-transformed clip rect. Text is rasterized at 1× and scaled, so it is soft below about 0.6 on a 1× display and blurry past 1 (egui issue #4813), hence the cap; one text size also ends the font atlas stalls. Two windows on one device: with `Fifo` on both, a minimized window held both to about 31 fps (120 Hz display); `Mailbox` on the hidden one kept both at 120. The stutter is accepted, since windows redraw only on change. On Wayland Drip cannot pin windows, place them or detect occlusion, and winit 0.30 has no parent windows there; Drip sends `xdg_toplevel.set_parent` itself on winit's connection, through `wayland-client` (already in winit's tree). AccessKit bounds ignore layer transforms in egui 0.36, so tests click canvas widgets by AccessKit action. |

## 7. Milestones (proposed)

1. **M1: graph engine and persistence.** Types, kinds and registry, graph editing with type and cycle checks, pull evaluator with stamps and cache, views and actions, the JSON format with migration and opaque nodes. Tested headlessly with toy node kinds.
2. **M2: prototype pipeline.** LibRaw shim and binding, the six processing nodes plus histogram and preview views, LittleCMS export. Ends with a harness that turns a raw file into a TIFF. Tests: synthetic mosaics with known matrices; real raw fixtures (CC0 samples) checked against normalization and matrix values computed independently; TIFF round-trips that check the embedded ICC profile and the values.
3. **M3: GUI.** Display spike, then the toolkit decision, then node editor, parameter panels, preview and histogram, export action. KWin surface tagging is part of this milestone, because the chosen GUI's surface depends on it.
4. **M4: display color path completed.** The app-side fallback, monitor changes, measured verification.
5. Later: CLI implementation, then the deferred items in the handoff.

## References

[1] D. Coffin, "dcraw.c," `convert_to_rgb()` and `cam_xyz_coeff()`. [Online]. Available: https://www.dechifro.org/dcraw/

[2] Tracel, "CubeCL," project documentation. [Online]. Available: https://github.com/tracel-ai/cubecl. Accessed: Oct. 4, 2026.

[3] Tracel, "CPU runtime for CubeCL," v0.11.0-pre.4, dependency manifest. [Online]. Available: https://docs.rs/crate/cubecl-cpu/0.11.0-pre.4/source/Cargo.toml. Accessed: Oct. 4, 2026.

[4] Tracel, "LLVM Compiler," v0.11.0-pre.4, dependency manifest. [Online]. Available: https://docs.rs/crate/cubecl-llvm/0.11.0-pre.4/source/Cargo.toml. Accessed: Oct. 4, 2026.

[5] Rayon contributors, "ThreadPoolBuilder," API documentation. [Online]. Available: https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html. Accessed: Oct. 4, 2026.

[6] Rust GPU contributors, "spirv_builder," API documentation. [Online]. Available: https://rust-gpu.github.io/rust-gpu/api/spirv_builder/. Accessed: Oct. 4, 2026.

[7] C. R. Earp, "krnl: Safe, portable, high performance compute (GPGPU) kernels." [Online]. Available: https://github.com/charles-r-earp/krnl. Accessed: Oct. 4, 2026.

[8] Tracel, "CPU thread pool," CubeCL v0.11.0-pre.4 source. [Online]. Available: https://docs.rs/crate/cubecl-cpu/0.11.0-pre.4/source/src/compute/threadpool/mod.rs. Accessed: Oct. 4, 2026.

[9] Apple, "Transition to Metal." [Online]. Available: https://developer.apple.com/opencl/. Accessed: Oct. 4, 2026.

[10] Khronos Group, "SPIR-V." [Online]. Available: https://www.khronos.org/spirv/. Accessed: Oct. 4, 2026.

[11] W3C, "WebGPU Shading Language," sec. 15.7, "Floating Point Evaluation." [Online]. Available: https://www.w3.org/TR/WGSL/#floating-point-evaluation. Accessed: Oct. 4, 2026.

[12] Tracel, "CubeCL wgpu runtime," v0.11.0-pre.4. [Online]. Available: https://docs.rs/crate/cubecl-wgpu/0.11.0-pre.4/source/src/runtime.rs. Accessed: Oct. 4, 2026.

[13] Tracel, "Tracel LLVM," build and distribution documentation. [Online]. Available: https://github.com/tracel-ai/tracel-llvm. Accessed: Oct. 4, 2026.

[14] Apple, "Porting just-in-time compilers to Apple silicon." [Online]. Available: https://developer.apple.com/documentation/apple-silicon/porting-just-in-time-compilers-to-apple-silicon. Accessed: Oct. 4, 2026.

[15] Rayon contributors, “ParallelIterator,” Rayon 1.12.0 API documentation. [Online]. Available: https://docs.rs/rayon/1.12.0/rayon/iter/trait.ParallelIterator.html. Accessed: Oct. 4, 2026.

[16] egui contributors, “Context::request_repaint,” egui 0.36.2 API documentation. [Online]. Available: https://docs.rs/egui/0.36.2/egui/struct.Context.html#method.request_repaint. Accessed: Oct. 4, 2026.

[17] winit contributors, “EventLoopProxy::send_event,” winit 0.30.13 API documentation. [Online]. Available: https://docs.rs/winit/0.30.13/winit/event_loop/struct.EventLoopProxy.html#method.send_event. Accessed: Oct. 4, 2026.
