# Progress log

Newest first. Each entry: what happened, what is verified, what is next.

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
