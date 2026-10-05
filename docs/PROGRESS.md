# Progress log

Newest first. Each entry: what happened, what is verified, what is next.

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
