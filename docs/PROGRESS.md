# Progress log

Newest first. Each entry: what happened, what is verified, what is next.

## 2026-10-05: per-node GUI and pop-out windows

- Decided with the user (G13): each node kind gets a frontend GUI drawing real widgets on a transformed canvas, and nodes can pop out into OS windows arranged by the window manager. Zoom is capped at 1; pop-out state is not saved; no docking.
- Spikes, discarded afterwards: an offscreen render with simulated input confirmed widgets, popups and previews on a transformed egui layer, with two fixes needed (transformed callback rect, inverse-transformed clip). Two windows on one device showed a minimized `Fifo` window holding both to about 31 fps; `Mailbox` on the hidden window avoided it. `Fifo` stays; alternatives are in TODO.
- `Display` is split into the shared `Gpu` and per-window output. The editor draws on an egui layer transformed for pan and zoom, in graph units; text no longer re-rasterizes per zoom, which ends the font atlas stalls (TODO row removed).
- `gui::NodeGui` per kind draws the body below the frame; preview and histogram nodes keep their saved size before a result arrives and draw their view. Node GUIs and the inspector edit through `NodeCx`; `App` knows no node kinds.
- Windows show one part of one node (`gui::Popped`). The frame shows ⚙ on nodes with parameters, popping out the inspector's view of the node; viewer bodies show 🗗 over the view's corner, popping out the view. A first version chose one window per node by kind, which left the histogram's parameters unreachable; the user pointed this out.
- The shell opens and closes windows to match, and redraws all windows only when the graph, results or pop-outs change (idle with two pop-outs: 9 frames in 8 s).
- Pop-outs are children of the main window, so KWin keeps them in front of it (the user's choice over always-on-top, which Wayland does not offer). winit has no parent windows on Wayland, so `parent::set_parent` sends `xdg_toplevel.set_parent` on winit's connection; `WAYLAND_DEBUG` showed the request with no protocol error. Windows and macOS are in TODO.
- Resizing any window stalled for a second at a time since vsync (36a6edc): during an interactive resize KWin takes no buffers until the client answers its latest configure, extra frames filled the `Fifo` swapchain, and the blocked image acquire kept Drip from answering until wgpu's 1 s timeout. `pre_present_notify` now lets winit hold redraws until the compositor asks for a frame. In the user's resize log: 878 resize events and 293 frames, the slowest 3.9 ms, no timeouts (before: three 1 s stalls).
- Panning failed above and left of the nodes: egui registers the canvas `Ui` as a hover widget over the area its contents span, which hid the pan response in the layer behind it. The canvas `Ui` now senses clicks and drags itself and spans the visible area, as `egui::Scene` does; a test clicks empty canvas above the nodes. The user checked panning and node interaction.
- Review of the session's code, then cleanup in separate commits:
  - `Frame` carries a `Report` of what node GUIs did; `NodeCx` no longer exposes its id or frame for writing; `App::redraw` and `Report::edited` replace two flags both called `changed`.
  - The shell polls the worker on its wake event; before, results were taken only inside a window's frame, so popped-out windows went stale while the main window could not draw.
  - `widgets` holds the pop-out button, the resize handle and `point`, the one way to undo the canvas zoom.
  - The main window's display reuses the surface that picked the adapter.
  - The viewer reads its saved size in one place.
  - `Editor::show` keeps interaction; canvas setup and frame painting have functions of their own.
  - Parameter windows open at the size their content takes (sigmoid 296×91, export 296×238 points), replacing a guessed `80 + 26 × parameters` height. egui has no measuring pass, so `App::window_size` lays the content out once in a scratch context at the window's scale. A first version resized the window after its first frame; KWin's next configure restored the creation size, so measuring has to come before the window opens.
- Canvas zoom goes up to 2 at the user's request (was 1); text blurs past 1, previews stay sharp. Fitting a project still stops at 1.
- A render that looked changed after polling moved to the shell came from the render helper, which drove `App::ui` without polling; with polling, the render matched the one before the cleanup pixel for pixel.
- Verification: 13 GUI tests (two new: ⚙ and 🗗 counts, opening both histogram windows, closing and project replacement; parameter window content and removal), formatting and clippy pass; offscreen renders of the default template and a live run with the histogram's parameters and view popped out (three scRGB surfaces, both parented) looked right.

## 2026-10-04: histogram range and labels

- Histogram nodes take `min_ev` and `max_ev` (G12); the plot labels 0 EV, and its labels use the editor's zoomed port font. Projects saved before this lack the two parameters and no longer load (F6, no migration).
- The empty region above 0 EV is not clipping: the template's histogram reads the sigmoid output, which maps scene values into [0, 1).
- Integer parameters get the same slider as floats in the inspector instead of a drag value.
- A `scale` parameter plots counts linearly or on a log scale (default), replacing the square root; the choice travels in `Histogram::log`.
- Verification: workspace tests (including custom bounds and scale), formatting and clippy pass; the user checked the GUI.

## 2026-10-04: balanced default layout

- The built-in template places a 3:2 preview (672×440) beside the end of the processing chain, the histogram (320×240) beyond it and the exporter under the chain, following the user's arrangement.
- Opening a project now fits the whole graph, view sizes included, and zooms out as needed (about 0.8 maximized, 0.62 in the default window); it never zooms in past 1. Before, it centered node corners at zoom 1.
- Verification: workspace tests, formatting and clippy pass; the user checked the layout in the GUI.

## 2026-10-04: frame timing log

- Each frame logs its UI, tessellation and render time and egui's font atlas fill at debug level under the `frame` target (`RUST_LOG=frame=debug`). Evaluation timings were already logged per node. Motivation: fast zooming lags slightly, suspected to be node text re-rasterized at every new `13.0 * zoom` font size.
- Verification: formatting and clippy pass; a debug run prints sub-millisecond idle frames.
- Confirmed in release logs: a fast zoom stalls one frame each time egui's font atlas doubles (up to 274 ms at 50% full), since every frame lays text out at a new `13.0 * zoom` size. Rounding font sizes to whole points stopped the growth but looked wrong and was dropped.
- Surfaces now request `Fifo`; wgpu's default took the driver's first mode, which rendered 500–800 frames per second. With vsync, frames hold at about 120 per second, but zooming still grows the atlas (32% in a few seconds, 136 ms stall at 25%).
- The remaining zoom stalls are accepted for now; the text-scaling options are deferred in TODO.

## 2026-10-04: GUI decoupling

- Moved view preparation out of `worker`: `preview::Image` packs `Rgba16Float` texels next to the upload that consumes them, and `views` owns `PreparedView` and the prepared-image cache. The worker still runs preparation and owns the cache; it is now only the mailbox and evaluator host.
- The inspector now takes the graph and an `inspector::Frame` that reports edits, refusals and requested actions, matching the editor's `Frame`. `App` applies them after drawing and exposes nothing to other modules.
- Verification: all 11 GUI tests, formatting and clippy pass after each commit.

## 2026-10-04: consume the export evaluator

- `run_action` consumes its evaluator, using its resource store and a fresh temporary node cache. The worker forks once before dispatch; removed the second fork inside the action. Standalone actions need no fork.
- Verification: all 74 workspace tests, formatting and clippy pass, including full-detail actions, preview-cache preservation, resource sharing/invalidation, TIFF output and intermediate release. The three cleanup items are complete in separate commits; no new deferred work.

## 2026-10-04: remove eager RAW pyramids

- Cache decoded RAW once per resource store; derive the requested normalized level using the existing normalization/downsampling algorithms and discard intermediates. Removed E17 from TODO.
- Verification: workspace tests, formatting and clippy pass. Tests check independent CFA averages, LibRaw agreement, TIFF output, old-level release and level/fork reuse after deleting the source file.
- Release benchmark: default 1/2 level changes 45.0→244.1 ms; Full 189.0→358.0 ms. Resource storage is about 85 MB instead of 226 MB, excluding node results and temporary buffers. Full measurements and limits are in DESIGN 3.3.7. Export snapshot cleanup follows separately.

## 2026-10-04: simplify resource invalidation

- Removed per-file reload/revisions and resource-path enumeration. Preview/export share a resource store, including concurrent first reads; invalidation and project replacement create a fresh store while existing exports retain their readers.
- Verification: workspace tests, formatting and clippy pass. Tests retain cache reuse across levels, fresh reads after invalidation/project reset, shared first loads and export lifetime checks. RAW level retention and duplicate export snapshot cleanup follow separately.

## 2026-10-04: remove visibility-based evaluation

- The GUI now requests every graph node when computation changes. Removed viewport target selection, the editor's stored viewport, target-set comparison and partial presentation merging. Canvas navigation and node geometry no longer schedule evaluations; export still targets its own dependencies at full detail.
- Verification: all 11 GUI tests, formatting and workspace clippy pass. A regression test places disconnected nodes far off-screen, verifies evaluation and parameter updates, and checks that moving a node schedules no work. No new deferred work.

## 2026-10-04: half-scale preview default

- Changed the default global preview detail to 1/2 as requested. Applies to new projects and projects without a saved level; explicit saved settings still take precedence.
- Verification: GUI tests and formatting pass, including default preview dimensions and resetting a new project. No new deferred work.

## 2026-10-04: invalidate the full evaluation cache

- Replaced the button's per-file reload with a fresh evaluator on the worker, clearing every node result and loaded resource. Earlier completions are rejected and pending requests discarded; the UI requests visible targets again while retaining the last presentation. Existing exports retain their snapshots; subsequent exports use the fresh resource store.
- Verification: GUI tests cover recomputing nodes without files, rejecting in-flight results, discarding pending work, fresh file reads, export ordering and clicking the button to regenerate identical preview pixels. Formatting and clippy pass. No new deferred work.

## 2026-10-04: cache button wording

- Renamed “Reload files” to “Invalidate cache” as requested; the existing resource invalidation behavior is unchanged.
- Verification: formatting and diff checks pass. No new unimplemented work.

## 2026-10-04: background previews and evaluation status

- User approved the worker and requested the existing lowercase message style. Added “evaluating…” while preview work is pending, retaining the previous image; current completion shows “done” or the error. Export retains “running export…” and its separate full-detail execution.
- `drip-gui::worker` owns the existing synchronous evaluator. Requests name target nodes and the global detail level; one request runs while only the latest pending request is retained. Monotonic generations reject stale results, including after project changes that reuse node IDs. Successful parameter/topology edits and visibility changes schedule requests; ordinary canvas/layout edits do not recompute images. Off-screen nodes keep one last presentation so their layout cannot collapse and oscillate the visible target set.
- Reset/reload/export commands remain ordered. Resource snapshots for export are now taken on the evaluator-owning worker, then executed on the existing independent export thread. The library, node APIs, Rayon kernels and RAW cache policy are unchanged.
- Moved RGB f32→RGBA f16 preparation off the UI thread. The worker publishes views/errors without intermediate outputs and retains prepared allocations until renderer/UI references are released, then collects them off-thread. Wgpu upload and drawing remain on the render thread. Winit user events wake the window for completion and worker failures; no polling timer or async runtime was added.
- Verification: all 73 workspace tests, formatting and clippy pass. New tests cover a blocked node with responsive controls and visible lowercase status, retaining the last image, latest-target coalescing, stale project results, error replacement, cache reuse, reload/export ordering, full-detail actions, worker panic notification and prepared-image ownership/bytes. A live Wayland/RTX 3080 smoke run opened the fixture at Full detail, completed background evaluation and rendered without reported GPU validation errors; the run was ended by its eight-second timeout.
- Remaining work: RAW pyramid simplification (E17), optional cooperative cancellation, and end-to-end driver upload/draw measurements. A running evaluation finishes before pending work starts; the implementation makes no frame-time guarantee for GPU uploads. Design 3.3.6 and TODO record these limits.

## 2026-10-04: investigate the async worker

- After committing manual global detail as `ec2fa58`, traced evaluation ownership, UI edit paths, presentation data and the window loop. The library API can stay synchronous and unchanged; the adapter belongs in `drip-gui`, with explicit target IDs and the single global preview level.
- Found that the current window loop has no background wake-up bridge. An egui repaint request alone cannot wake this integration. Proposed a winit `WorkerReady` user event following result enqueueing, with notifications on worker termination as well; verified the pinned egui/winit sources and documentation.
- A temporary probe outside the repository ran the existing evaluator on a persistent std thread through all four measured levels. It also measured the exact current RGB f32→RGBA f16 preparation: median 1.3/6.3/21.5/118.4 ms at levels 3/2/1/0 over seven release repetitions. This excludes GPU upload/drawing and confirms preparation must also move off the UI. The probe is not committed.
- Recorded concrete module boundaries and remaining validation in DESIGN 3.3.5; updated TODO. No worker, scheduler or cache implementation changes. Diff checks pass; the preceding global-detail commit passed all 68 workspace tests, formatting and clippy.

## 2026-10-04: manual global preview detail

- Implemented the global Preview detail selector (Full through 1/256, default 1/8), persisted as `Project::ui.preview_level`. Invalid saved levels fail at project loading without replacing the current project. New projects return to the default.
- Removed automatic level selection and the image-dimension feedback from drawing. The existing `evaluate(graph, level, targets)` API and one-result-per-node cache are unchanged. Preview/histogram targets share the selected level; export continues using its separate full-resolution action path.
- Validation: the headless GUI test operates the control, checks image scale and histogram pixel counts, verifies resizing preserves the result, saves/reopens the setting and resets it with New. Invalid level values are rejected. The harness now uses the application's 1600×1000 initial size so requested view targets stay visible. Workspace tests, formatting and clippy pass.
- This commit changes detail selection only. RAW cache simplification remains pending; the next step is investigating the async worker, not implementing it yet.

## 2026-10-04: clarify evaluation targets

- User emphasized that evaluation is always against a particular node. Made target identity explicit in the worker plan: a request names target NodeIds, while the graph supplies dependencies and the level controls resolution. Batching targets shares their upstream work; coalescing must preserve the complete desired target set. Export names its node and action separately and forces level 0.
- Documentation only; no evaluator or scheduling changes. Diff checks pass. The architecture remains a plan for review.

## 2026-10-04: plan preview and worker architecture

- After committing the Rayon replacement as `c6857e5`, inspected the frontend/evaluator boundary and recorded a separate tentative plan in DESIGN 3.3.4. No processing, level, cache or scheduling code changed in this step.
- Current behavior: the GUI requests all visible nodes at one level; nodes are passive, the evaluator walks ancestors and passes `EvalContext`. Export already evaluates at level 0 on a separate background thread. These boundaries can remain.
- Requested next behavior: one manual global preview level, replacing the earlier per-preview proposal, and removal of eager RAW pyramid retention. Proposed implementation keeps normal node-result caching and active-project decoded-resource sharing; persistence and resource lifetime details remain a plan for review.
- Proposed async ownership: a persistent preview worker owns the evaluator; one active/one latest pending preview request, project/generation checks, ordered reset/reload/export commands, and the existing export worker. GUI-side texture packing and large-buffer retirement must also leave the UI thread. Cancellation between nodes is deferred unless measurements justify it.
- Validation for the future implementation is listed in the plan. This step is documentation only; diff checks pass. The Rayon implementation remains the tested state, with automatic levels and RAW pyramids still active.

## 2026-10-04: replace CubeCL with Rayon

- User selected Rayon and requested replacing the current CubeCL commit. White balance, debayer, color matrix, sigmoid and histogram now use one ordinary Rust implementation each, with Rayon's shared CPU pool. Removed CubeCL, the dispatch/transfer layer and compiler dependencies; restored MSRV 1.92. No experiment files or second kernel versions remain. GPU computation is postponed (E15).
- The histogram's initial per-pixel array fold was costly: the first 12-worker level-0 measurement was 352 ms. Accumulating in place per chunk reduced the final complete graph median to 162 ms. Production kernel/dispatch code is about 105 lines instead of 390; the stripped benchmark is 0.84 MiB instead of 105.3 MiB.
- Identical release benchmark, Sony fixture, Ryzen 5600G/12 threads and RTX 3080/Vulkan, seven warm level 3/2/1/0 cycles: Rayon 12 workers 53/9/37/162 ms; Rayon 1 worker 61/55/244/1038 ms; saved CubeCL 0.11 CPU binary 51/24/130/582 ms; GPU 53/28/123/623 ms. Initial RAW+runtime evaluations were 458/480/619/1067 ms respectively, with uncontrolled filesystem/driver caches. GUI conversion/upload/drawing are excluded. RAW cache and evaluator are identical in all comparisons.
- Correctness and validation: all 67 workspace tests pass; 21 kernel/node tests also pass with one Rayon worker. Covers f64 numerical references, exact histogram boundaries, concurrent calls, real LibRaw comparison and TIFF export. Workspace formatting and clippy with warnings denied pass.
- Scope remains node internals only. The latest user instruction defers preview-level, evaluation, cache and async changes until after this implementation; next is a separate architecture plan. Current automatic level selection and RAW pyramid retention remain in place.

## 2026-10-04: upgrade CubeCL to 0.11 prerelease

- Upgraded and pinned CubeCL to `0.11.0-pre.4` as requested. Compute, GUI and egui now resolve to one wgpu version, 30.0.1. Adapted the wrapper to runtime-independent `Client`/`BufferArg` APIs, slice kernel arguments and explicit resource server types; kernel formulas, tolerances and host-valued node boundaries are unchanged. Raised workspace MSRV to Rust 1.95, required by the prerelease.
- The CPU backend now uses Pliron with LLVM 23.1 instead of the previous MLIR backend. Direct input-buffer writes remain to avoid allocating temporary host vectors. The stripped Linux compute benchmark is 106 MiB (previously 112 MiB), with no dynamic LLVM dependency.
- Verification: all 67 workspace tests pass with CPU and with GPU selected; 21 kernel/node checks pass with one CPU worker. Workspace clippy with warnings denied, formatting and diff checks pass. This includes real RAW/LibRaw comparisons, export, histogram boundaries, extreme finite sigmoid inputs and concurrent evaluations.
- Release fixture medians at levels 3/2/1/0: CPU 44/25/121/555 ms; GPU 56/31/133/638 ms. First RAW+runtime evaluation was 0.66 s CPU / 0.79 s GPU, with uncontrolled filesystem/compiler caches. Levels 1–3 remain below 300 ms; full resolution remains above it. The cold dependency rebuild took several minutes; benchmark timings were collected after builds/tests finished.
- Amends the production integration commit at the user's request. No experiment files or additional kernel versions were introduced. Windows/macOS packaging and full-resolution latency remain in TODO.

## 2026-10-04: integrate CubeCL inside existing nodes

- User approved the runtime/compiler footprint, lowered the Windows/macOS priority, and set 300 ms as acceptable preview latency. Integrated pinned CubeCL 0.10.0 into `drip`; removed `experiments/` and its independent package. White balance, 2×2 debayering, camera-to-Rec.2020, sigmoid and histogram now each have one CPU/GPU kernel source. RAW preparation, graph/evaluator/cache contracts, host values, renderer and export implementation are unchanged.
- `DRIP_COMPUTE=auto` prefers a hardware wgpu/WGSL adapter, with CPU fallback; `cpu`, `cpu1`, and `gpu` allow explicit selection. Buffer sizes exceeding device binding limits also select the same CPU kernel. Runtime resources are reused. Direct writes to fresh CubeCL allocations remove redundant host upload copies; output is copied once into each node's host value. No cross-node device residency or fusion.
- Histogram bin edges are shared host-generated thresholds so CPU/GPU agree exactly at boundaries. The stable sigmoid is tested against an independent f64 curve across 25 parameter combinations and extreme finite values, with 2e-6 absolute tolerance. Added checks for all Bayer phases, odd crops, dispatch/SIMD tails, empty images and concurrent evaluations.
- Release fixture measurements (Ryzen 5600G, 12 threads; RTX 3080/Vulkan), seven warm cycles through levels 3/2/1/0: CPU medians 44/36/133/525 ms; GPU 49/38/138/618 ms. The preceding committed Rust implementation, rebuilt separately with the same benchmark, measured 58/50/235/1001 ms. These include node transfers, allocations and cache replacement, excluding GUI texture upload/drawing. Levels 1–3 meet 300 ms; full resolution remains above it. First RAW+runtime evaluation was 1.13 s CPU / 0.69 s GPU, with uncontrolled caches. Reproduction is `examples/preview_latency.rs`.
- Verification: all 67 workspace tests pass with CPU and with GPU selected; all 21 kernel/node tests also pass with one CPU worker. Automatic fallback was exercised with only a software adapter available. Workspace fmt and clippy with warnings denied pass.
- The stripped Linux compute benchmark is 112 MiB, with no dynamic LLVM/MLIR dependency. Windows/macOS packaging, other GPU vendors, and device-loss recovery remain unverified. Updated DESIGN E10/E14, usage and TODO; the preliminary experiment was not committed.

## 2026-10-04: acceleration scope clarified

- User requires acceleration to stay inside selected nodes and remain transparent to other infrastructure. Recorded E13: preserve host-value inputs/outputs, evaluator, graph, caches and renderer, with one source per accelerated kernel across backends. Mixed ordinary/accelerated nodes are expected.
- Revised the current plan and experiment conclusion: the existing tone-map wrapper has not shown a consistent whole-node gain. That measurement does not rule out faster isolated nodes. Optimize internal copies/runtime reuse and measure complete calls; cross-node device residency and renderer integration are outside this change. Background evaluation remains a separate proposal.
- Documentation only; no processing changes or commits.

## 2026-10-04: CubeCL distribution follow-up

- User instructed not to commit experiments. Removed experiment commit `e77bf15` from the branch while preserving its files and documentation uncommitted; the RAW-cache commit `1a5928c` remains.
- Inspected the installed Tracel LLVM/MLIR build scripts: LLVM/MLIR link statically. The combined Linux binary has no dynamic LLVM/MLIR dependency; a stripped copy passed CPU checks with no compiler tools on PATH. Its stripped size is 115 MiB, versus 8.4 MiB for the wgpu-only experiment. These are harness sizes, not final application estimates. Existing LibRaw/LittleCMS/system dependencies remain dynamic.
- Distribution remains unverified on clean Windows/macOS machines. The CPU JIT needs a signed macOS/Apple-silicon check against Apple's executable-memory requirements. Native per-target CI builds, ordinary-driver GPU checks and same-source CPU fallback remain proposed. Details were in the uncommitted experiment report, since replaced by DESIGN 3.3.2; no application changes or new commits at that point.

## 2026-10-04: one-source CubeCL kernel experiment

- Committed the prior RAW-cache work as `1a5928c` after the user's reminder. Read CubeCL 0.10.0's book, examples, CPU compiler/runner, wgpu runtime, memory and profiling code; the inspected tag is `7cf203735e095e640a2c03b2400d0faa03196bb4`.
- Added the independent `experiments/cubecl` package, with a pinned dependency/lockfile, one sigmoid kernel, a harness calling the existing production node as the oracle, reproduction commands and retained measurement logs. No application code or normal-workspace dependencies changed. The kernel uses logarithmic algebra to avoid relying on infinities under WGSL's finite-math rules.
- Correctness passed on Linux Ryzen 5 5600G with 1/12 CPU workers and RTX 3080 through both WGSL and direct SPIR-V on Vulkan. Checked 25 parameter combinations, empty/odd/tail lengths, negative/zero/subnormal/extreme finite values, and Sony fixture scene RGB at levels 3/2/1/0. Maximum error: 7.75e-7 synthetic, 1.79e-7 fixture, against tolerance 2e-6. Native wgpu writes into CubeCL buffers also passed on both GPU routes.
- Release medians at 664k RGB pixels: existing Rust node about 12 ms; resident CubeCL CPU 1/12 workers 10.0/2.2 ms; resident GPU about 0.11 ms including synchronization (device timestamps 0.026 ms). Complete host-slice→host-Vec calls took 13–24 ms, erasing the gain. Full-size GPU host latency varied substantially; the report keeps those results separate from device timings and does not claim application speedups.
- Initialization took roughly 0.25–0.35 s, plus CPU first-use work about 0.25 s or GPU first-use work 0.003–0.020 s, with driver/compilation caches uncontrolled. First combined dependency build succeeded in about 88 s with the automatically provisioned LLVM bundle. CubeCL's CPU `CUBE_COUNT` builtin was unsupported; its one-dimensional form worked without dependency patches.
- Format and clippy checks cover the experiment's GPU and CPU configurations. The standalone CPU-only build also works. Remaining adoption questions: Windows/macOS packaging, other vendors, CFA/reduction kernels and retaining buffers through the pipeline/preview. CubeCL uses wgpu 29 while Drip/egui use 30. Tentatively prefer WGSL for the next experiment; no production backend selected (E10, DESIGN 3.3.2).

## 2026-10-04: cache RAW pyramids for the session

- Implemented E11 after user approval: normalize each RAW once, eagerly build the Bayer-preserving pyramid and release the decoded u16 buffer. The generic resource store shares each file revision across evaluators; export snapshots retain their cached content through reload. The GUI retains resources across project changes. Evaluation stamps, ordering and node-result caching are unchanged.
- Verification: 61 workspace tests pass, including independent direct averages with patterned black, all four Bayer phases and cropped edges; reuse of the same RAW allocations across levels/readers/evaluators; concurrent first loads; export-first resource sharing; reload isolation and release of old resources; existing LibRaw reference and TIFF checks. Clippy passes with warnings denied.
- Fixture probe (same dev opt-level 1 setup as below, single-run timings excluding rendering/upload): cached `raw.read` now takes 1.6–2.2 µs. Level 3→2 graph evaluation fell from 215 to 78 ms, and 2→3 from 145 to 20 ms. Initial level-3 evaluation increased from 400 to 501 ms because all RAW levels are prepared upfront. Downstream nodes still recompute on level changes; level 0 remains about 1.2 s.
- Scope: no dependencies or GPU/background-evaluation work added. The persistent pyramid is about 226 MB for the fixture; retained files accumulate until reload or application exit, as agreed for the prototype.

## 2026-10-04: preview performance investigation

- User requirements: cross-platform support and ease of development take priority over peak kernel performance; strictly one source implementation per computational kernel across execution backends (E9, also recorded in AGENTS.md).
- Traced the lag: `App::ui` evaluates synchronously. Decoded u16 RAWs already survive level changes in `Resources`, but `raw.read` normalizes and bins almost the entire sensor again at every new level. The evaluator retains only one result per node, so returning to an earlier level recomputes it. Export uses a fresh evaluator and resource store, and opening/new projects drops the previous store. Preview uploads also convert RGB f32 to RGBA f16 on the CPU; that cost was not measured here.
- Measured with a temporary external probe calling the existing library, on the 7968×5320 Sony fixture, Ryzen 5 5600G, dev opt-level 1: decode 255 ms; standalone normalize at levels 3/2/1/0 about 136/147/191/335 ms. With decoded data cached, graph evaluation from level 3→2 took 215 ms, 2→3 took 145 ms (130 ms in `raw.read`), level 1 took 488 ms and level 0 about 1.5 s. Repeating the unchanged level took 9 µs and evaluated zero nodes. These are diagnostic single-run wall times, excluding rendering/upload; the filesystem cache was uncontrolled, so decode timing is not a cold-disk measurement.
- Surveyed upstream documentation and source: Rayon covers one-source serial/parallel CPU work; CubeCL is the leading candidate for one-source CPU/GPU execution, with an LLVM/JIT CPU dependency and evolving APIs. rust-gpu/krnl require more compiler/runtime integration; separate WGSL and Rust algorithms violate the user's requirement; OpenCL is deprecated on macOS. Details and references are in DESIGN 3.3.1.
- Proposed, not selected or implemented: persistent background evaluation (would revise G2), cached normalized CFA levels, sharing decoded resources with export, and a small CubeCL portability/numerical/latency experiment (E10–E12). At this fixture size the decoded samples occupy 84.8 MB; a full f32 CFA pyramid would occupy at most another 226.1 MB before downstream images, so retention needs a policy.
- Verification: the probe completed all requested levels and checked both view targets for evaluation errors. No processing code or dependencies changed. Next: settle the backend direction and caching/worker scope before implementation.

## 2026-10-04: external parameters and the canvas

- User decisions: any parameter can be external, with schema defaults and one input per node (F7); nodes are identified by id with renamable labels; the editor becomes the main canvas with views drawn in nodes, zoom and resize (G7); enlarging into a separate window is postponed; zero compatibility in the prototype (F6).
- Library: nodes hold `&'static NodeKind`, the evaluator works on the graph alone (no more eval/project cycle), the file format is private to `project` and strict, actions get the evaluation context, stamps hash canonical JSON, templates live in `templates`.
- GUI: canvas with pan/zoom, inline image and histogram views, resizable nodes, evaluation of visible nodes only.
- Fixed: a segfault on closing the window (the Vulkan surface outlived the Wayland connection; confirmed with exit status 139 before and 0 after), previews squashing when clipped (reported by the user), preview textures leaking, off-screen previews forcing finer levels. The last three were found by Codex's review.
- Verified: 58 tests, every commit builds and passes on its own; GUI ran on the fixture with no wgpu validation errors.
- Note: per-commit checks in worktrees must use their own target directory; sharing it left a test binary pointing at a deleted worktree.

## 2026-10-04: M3 (GUI)

- Display path measured (D5): on KWin/NVIDIA, scRGB gives the same pixels as tagging the surface Rec.2020, and matches sRGB within rounding in gamut. Beyond sRGB still needs the colorimeter check (TODO).
- `drip-gui`: winit + wgpu + egui with an offscreen gamma canvas and a final scRGB pass (sRGB fallback with a warning), wide-gamut previews via paint callbacks, a custom node editor over the project graph, schema-driven inspector with bindings and graph inputs, histogram, export on a worker thread, open/save project and template. Neutral middle-grey style (G5). Logging through `log`/`env_logger`.
- Codex reviewed the GUI and found 7 issues, all fixed: a continuous redraw loop, preview-level oscillation, lost label edits, NaN offset on an empty graph, lost surfaces, incompatible bindings (now refused by the library), filename changed by a failed Save As.
- User decisions: no migration (F6; files carry the major version); built-in output profiles sRGB, Display P3 and Rec.2020 (C7).
- Verified: 57 tests, including 4 headless UI tests. The GUI runs on the fixture raw: decode 0.39 s once, re-evaluation from the sigmoid about 130 ms at the adapted preview level.
- Next: the user's review of the GUI; then the CLI, or remaining TODO items.

## 2026-10-04: M2 (prototype pipeline)

- `drip-libraw`: our own binding, a C shim with one safe `decode()`, linked to the system `libraw_r`. A `reference` feature exposes LibRaw's half-size pipeline for cross-checks. CI installs LibRaw per platform (Windows/macOS unverified, no remote yet).
- `drip`: file resources with explicit reload (stamps include path revisions); the node kinds `raw.read`, `color.white_balance`, `demosaic.bin2x2`, `color.camera_to_rec2020`, `tone.sigmoid`, `view.preview`, `view.histogram`, `export.tiff`; `examples/raw_to_tiff`.
- Verified: 53 tests. On two Sony ARWs (61 MP), Drip's scene-referred output matches LibRaw's independent pipeline with median relative error 1.3e-4 and p99 1.3e-3 over 14 M pixels; the global scale matches the predicted value. Export round-trips are checked against independently computed values. exiftool confirms the embedded ICC profile is byte-identical and typed UNDEFINED.
- Codex reviewed M2 and found 7 issues, all fixed with tests: an unsound buffer read in `reference()`, the black denominator for small patterns, the second green's multiplier, non-finite/singular metadata, a silently dropped final write in export, a concurrent double load, and sigmoid overflow.
- Added `fixtures/raw/sony-ilce-7rm3.arw` (Git LFS); the binding and pipeline tests now always run on it.
- Open for the user: confirm the tone curve (C4). Next milestone: M3, the GUI (winit + wgpu + egui), starting with measured verification of the scRGB display path.

## 2026-10-04: M1 (graph engine and persistence)

- The user approved the design. Their decisions: JSON; templates as functions (F5); no WB state in types (P6); "no CAT step" reading (C3); export as u16/f32 with no compression or deflate (C5); toolkit winit + wgpu + egui (D4). Commit format: `{submodule}: …` or `chore: …`. The three scaffold commits were reworded to match.
- Display spike: scRGB through the Vulkan WSI is tagged correctly on KWin. Self-tagging as Rec.2020 also works but relies on undocumented WSI behavior, so we use scRGB and the compositor does the display transform (D2).
- Implemented M1 in `crates/drip`: `value` (semantic port types), `param` (schemas, JSON values), `node` (kinds, registry, actions, migration), `graph` (validated editing), `eval` (pull evaluation, dependency stamps, per-node errors, full-resolution actions that release intermediates), `project` (graph inputs and arguments, JSON format, migration, opaque nodes).
- Codex reviewed M1 and found 11 issues. Fixed 10. Resource revisions (E2) are deferred to M2.
- Verified: 30 integration tests with toy node kinds. A mutation check confirmed the memory-release test catches regressions. clippy and fmt are clean.
- Next: M2. LibRaw C shim, raw.read with resource revisions, the prototype nodes, LittleCMS TIFF export.

## 2026-10-04: session 1

- Set up the workspace (`drip`, `drip-cli`, `drip-gui`), CI (Linux fmt/clippy/test; Windows and macOS build), AGENTS.md, DESIGN.md and TODO.md. License AGPL-3.0-or-later. git initialized, nothing committed yet.
- Surveyed LibRaw bindings: none is both sound and maintained (DESIGN L2). Proposed our own C-shim binding.
- Investigated the display path on the dev machine (DESIGN section 6): KWin is parametric-only and color-manages untagged surfaces as sRGB. NVIDIA's Vulkan WSI offers BT2020_LINEAR and scRGB float swapchains. GTK 4.22 didn't tag a Rec.2020 surface in a spike.
- Wrote the proposed core design (types, kinds and instances, pull evaluation with stamps, views and actions, persistence) for the user to review. Implementation waits until it is agreed.
- Codex (gpt-6-astra, high) reviewed the design. Its corrections are folded into DESIGN.md: matrix direction, black handling, limits of LittleCMS unbounded mode, scRGB and egui alpha details, resource revisions, port migration. Its open points are listed as open entries (P6, C3, C5).
- Next: user review of DESIGN.md, agree on milestones, then M1.
