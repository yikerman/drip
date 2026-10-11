# Progress

Validation evidence, not a feature inventory. Open gaps are in [TODO](TODO.md).

## 2026-10-11

- Files are read when a node is first forced and cached in the node, error
  included, instead of at admission. The GUI loading queue, pending bindings,
  "Reload files", `Kind::loads_assets` and the wait-for-loading action guard
  are removed; project open reads JSON only. Tests show a RAW decoded once and
  shared by graph snapshots after its file is deleted, and an ICC profile read
  at first use. Missing inputs are reported by the incomplete node's contract,
  so downstream nodes name it. Workspace tests, including 86 GUI tests, and
  strict workspace Clippy pass.
- Graph edits no longer run contracts. Connecting Demosaic to Orientation in
  the template was accepted without a RAW and refused with one, and selecting a
  RAW into that wiring was refused with an unattributed Sigmoid requirement
  error. Edits now check ports only; contracts run during evaluation, which
  names the failing node. `Outcome` and `Dag::description` are removed. A
  regression test wires and selects the RAW in both orders and reopens the
  project. CPU workspace tests, including 90 GUI tests, and strict workspace
  Clippy pass.
- CI run 38164895447 passed Linux and Windows but macOS timed out waiting for
  a dropped worker's preview completion. That wait, introduced in `31f2c18`,
  allowed five seconds despite sharing the CPU compute client with full-RAW
  tests. Compute completions now share the existing 90-second evaluation
  allowance; synchronization gates retain five seconds. Linux CPU GUI tests
  pass (90 passed, seven ignored), as do formatting and whitespace checks.
  macOS verification remains pending.
- Fixed parallel GPU-test startup by initializing the selected compute runtime
  before the display test's independent wgpu instance. CubeCL's existing startup
  serialization prevents concurrent first Vulkan calls. All seven GUI device
  tests pass in parallel on WGPU, including five fresh-process reruns, and on
  CPU. Strict GUI Clippy and workspace formatting pass. Application and editor
  behavior are unchanged.
- Isolated the parallel GUI device-test SIGSEGV to concurrent Vulkan extension
  enumeration during display/CubeCL startup. A standalone two-thread C probe
  crashes in 5/5 runs with only the NVIDIA ICD and passes 5/5 with lavapipe;
  disabling Mesa device selection does not prevent the crash. The core shows
  a null call inside `loader_icd_scan` while another thread initializes NVIDIA.
  Commit `4e44ccd` added compute scopes to the ignored GUI tests, which previously
  contained only the display test. This identifies the concurrency trigger by
  source history, not a bisected driver regression. Serial device tests pass;
  the recent refactor leaves the failing instance initialization unchanged.
- Separated the mechanical refactor from pending editor error-label, color and
  elision changes. The refactor-only snapshot passes all 14 editor interaction
  tests on CPU; the behavior changes and their test remain in the working tree.
- Mechanical simplifications: one core kind index and direct admission, core
  port handles in the GUI, typed template handles, shared canvas snapshots and
  cheap kind/name/layout reads. Removed redundant report/scope state, the GUI
  kind registry and view wrapper, the extra wake-callback allocation, retained
  preview source pixels, `Placement`, unused schema validation and integer
  scratch wrappers. Shared Bayer indexing and production rendering setup with
  tests. CPU workspace tests pass (159 passed, 16 device tests ignored), as do
  three doctests, strict workspace Clippy and formatting. All 16 device tests
  pass on WGPU with one test thread. A parallel GUI device-test process exited
  with SIGSEGV before reporting results; see the startup investigation above. Tests
  cover the combined tree before the user's remaining editor edits.
- Reviewed the 21 proposed simplifications against current code and design.
  Confirmed redundant report state, template label coupling, retained preview
  source pixels, repeated GUI snapshots and kernel/render duplication. Checked
  pinned winit/ecolor sources. Main corrections: GUI bindings are a separate
  registry; presentation flags alone leave stale prepared settings; auto
  exposure has a stricter requirement than Exposure; public export and
  evaluation boundaries still need checks. No implementation or benchmark run;
  recommendations remain tentative. Existing GUI edits were left untouched.
- Rechecked the simplification details: winit 0.30.13 returns an empty title
  on X11, which DESIGN excludes; Wayland retains its title. Worker events and
  notices differ for evaluation and shutdown. Core scales accept every integer
  from 1 to 256, while GUI levels select powers of two. Display tests use shipped
  shaders but duplicate packing and pipeline/binding setup. No tests or benchmarks
  run for this source review; concurrent tooltip edits were left untouched.
- Refreshed the README from the user-facing `37b13bf` revisions: CubeCL,
  flexible views, color management and pipeline checks, with brief canvas
  gestures and current build instructions. All five user-supplied Sony demos
  appear inline with captions, including an explicit gamut-warning caption.
  Inspected every image and verified local links, caption coverage, LFS
  attributes and whitespace. No processing code changed.
- CPU CI: reproduced run 38128023310's missing `Cpu` import in the RAW
  pipeline test and restored it within the feature-gated test. CPU workspace
  tests pass (157 passed, 16 device tests ignored), as do three doctests,
  strict workspace Clippy and formatting.
- Bindings and project open moved to their own loading thread per project:
  marking Preview and Export as asset loaders had queued their plain edits
  behind evaluations. GPU workspace tests, Clippy and formatting pass.
- Review findings: actions route by node with one resolved action per kind;
  one single-parameter merge; one batch evaluation call; auto exposure returns
  its parameters and shares the scopes' device image; a drop guard reports the
  worker's end. GPU workspace tests, Clippy and formatting pass.
- The app replaces its worker with the project instead of resetting it; the
  old thread skips its queue. New/Open wait for running actions, and actions
  for an opening project. GPU workspace tests, Clippy and formatting pass.
- Output ICC profiles load at admission as node assets; the per-path resource
  memo and worker cache invalidation are gone, and "Invalidate cache" became
  "Reload files". Actions and preparations receive the admitted node. GPU
  workspace tests, Clippy and formatting pass.
- Desktop capture check: launched `dist/drip-gui` on KDE Wayland, moved and
  resized its window, opened Preview detail through emulated pointer input,
  and inspected the captured menu. Temporary helper and captures are in
  `/tmp/drip-desktop-check.py` and `/tmp/drip-check-menu.png`; the test app
  and input device were closed. README/demo refresh remains paused for the
  concurrent code changes; use the Sony ILCE-7RM3 fixture when resumed.
- Review cleanups: one `#[default]` per choice; one typed-output check;
  test-only evaluation API removed; unread ports as `_name`; shared parameter
  JSON helpers; scope nodes beside their preparation; the RAW template ends in
  Export and the GUI template uses typed handles; schema-derived file lookup.
  GPU workspace tests, Clippy and formatting pass.
- Project format version now follows the `drip` major version (currently 0).
  GPU workspace tests, Clippy and formatting pass.
- Returned `drip-cli` to its scaffold and removed it from distribution builds.
  Projects lose the export target only it read; files that still carry it
  are rejected. The library template returns its rendered output instead. GPU workspace tests,
  Clippy and formatting pass.
- Model refactor cleanups: per-kind parameter widgets, coordinate labels from
  requirement labels, worker naming and the README's macro section. GPU
  workspace tests, Clippy and formatting pass.
- One worker queue: previews and actions are demands forced by one evaluator;
  actions no longer start a thread and runtime. Auto exposure meters Exposure's
  input as a queued action and applies its EV as an edit; an end-to-end test
  runs it twice on the Sony fixture. GPU workspace tests, Clippy and formatting
  pass.
- Implemented typed payloads and port requirements: one RGB payload, typed
  interpretations, requirement type arguments, contracts over known
  descriptions, admission-time parameter checks, device layouts only for
  kernel-read payloads, facts-only RAW metadata without rkyv, and merged
  Camera/Color reduction, histogram and waveform nodes. Workspace tests pass on
  the GPU including device tests; strict workspace Clippy and formatting pass.
- Typed parameter defaults and file roles: `Default` and schemas derive from one
  declaration, GUI file hints are gone, and histogram/waveform share a flattened
  exposure range. GPU workspace tests, Clippy and formatting pass.
- Moved TIFF export into `drip` as a node with a declared action; the CLI
  shares its `write`. GPU workspace tests, Clippy and formatting pass; the CLI
  exports the Nikon D70s fixture.
- Static node kinds, generated typed inputs and GUI bindings on node types.
  Workspace tests pass on the GPU including device tests; Clippy and
  formatting pass.
- Generated RAW as a `Load` source; `Node` lost `loads_assets` and its failing
  default `parameters()`. Same GPU test, Clippy and formatting checks pass.
- Resolved the model review's open questions with the user; decisions are in
  DESIGN under Model refactor. Documentation only; no code changes or tests.

## 2026-10-10

- Reviewed the model against `AGENTS.md` and the user's principles; recorded
  the gaps in DESIGN. Compacted agent documentation: kept the user's principles
  and the mathematics in TYPE_SYSTEM, removed model-authored redesign proposals
  and completed CubeCL migration records. Documentation only; no tests run.
- Waveform nodes default to logarithmic density for creation and reset; saved
  linear settings persist for both waveform nodes. GUI tests pass on CPU;
  strict GUI Clippy and formatting pass.
- Default template layout, wiring and persistence tests pass after the
  landscape rearrangement. Interactive layout review remains with the user.
- u8 TIFF export: all depths, uncompressed/Deflate output, saved settings,
  exact embedded ICC bytes and sRGB-encoded neutrals pass, as do proof/export
  parity, library project tests and strict workspace Clippy.
- CAT16: Colour 0.4.7 references pass on CPU and WGPU, including signed/HDR
  samples. Synthetic RAW neutrality, clipping levels, generated controls, saved
  overrides and ideal-XYZ-camera gain cancellation pass. The WGPU CLI exports
  the Nikon D70s fixture. The user's release Vulkan run on an RTX 3080 evaluated
  CAT16 and previews cleanly; it does not establish color accuracy or
  saved-project/export agreement.
- Device auto exposure: reference checks across partial reduction blocks and
  large HDR inputs, fixed-point and button tests pass on CPU and WGPU.
- Unified RAW metadata and orientation: 142 CPU workspace tests (15 device tests
  ignored) and four doctests pass; selected-backend WGPU checks cover gains,
  metadata transport, orientation and algorithm references. rkyv tests cover
  malformed buffers, exact float bits and coordinate mismatches. Sony lens and
  focal metadata match ExifTool. All eight orientations pass on WGPU, including
  single-row/column images and the omitted TIFF orientation tag.
- CI run 38046592913 timed out on CPU-backed GUI tests after the scope move:
  fixed 256-lane barrier groups oversubscribe the CPU runtime. Capped lanes at
  the reported limit; 132 tests pass locally including a two-core affinity run,
  and scope tests pass on CPU, Vulkan and WGPU.
- Device scopes: exposure counts match the CPU reference exactly; one of
  262,144 vectorscope samples changes bin versus f64. Clean WGPU baseline →
  device scopes: quarter 83.3→44.5 ms, half 187.5→68.0 ms, full 597.4→164.0 ms;
  Vulkan 82.6→45.4, 195.0→67.8, 571.9→170.6 ms (five samples each). Full-detail
  tracing: scopes 9.4 ms, packing 63.7 ms, GPU completion/readback 85.8 ms.
  Preview bytes match both backends and the baseline. Artifacts:
  `../drip-benchmark-results/2026-10-10-device-scopes/`.
- Parallel preview packing reduced half-detail preparation from 74–80 ms to
  7–23 ms in user Vulkan traces (uncontrolled interactions).
- RCD scratch lifetimes: peak Drip-owned scratch falls from eleven to eight f32
  lanes per pixel; RCD upstream vectors pass for all four phases on CPU and WGPU.
  The latest uniform WGPU run had peak RSS 2297.0 MiB (was 3240.5); sampled
  whole-GPU maximum rose from 1342 to 4325 MiB. Live allocations unmeasured.
- Export blocking during asset rebinding: invalidation, re-enable after commit
  and replacement-parameter regressions pass.

## 2026-10-08

- CPU-backed CI replaced temporary GUI test ignores after CubeCL device-service
  initialization failed on Linux CI run 37893323997. Hosted cross-platform runs
  remain unverified.
- The CubeCL rewrite passes the workspace suite on the real GPU, CPU-backend
  tests and strict Clippy. Opposed highlights match the f64 reference within
  9.54e-7 (CPU) and 1.55e-6 (WGPU) and pinned upstream vectors within 2e-5.
  RGB reduction matches an f64 reference over partial blocks, HDR and nonfinite
  values. Host storage reuse produced bit-identical preview bytes.

## 2026-10-07

- Linux workspace tests (165 passed) and strict Clippy pass after the
  dependency refresh; hosted Linux [run 37713841713](https://github.com/yikerman/drip/actions/runs/37713841713)
  passed. Rust 1.95 and Windows/macOS staging remain unverified.
- RAW decoding passes with LF and CRLF source manifests; upstream GNU/MSVC
  manifests list the same 79 translation units.

## Image and display evidence

- All 15 active RAWs across 13 manufacturers rendered without skips. Export
  matched full processing within one 16-bit code. Visual inspection found no
  structural corruption; cool/cyan/blue casts still need measured targets.
- TIFF metadata matched ExifTool/exiv2, with capture time checked under three
  time zones. Proof/export parity and gamut behavior passed with LittleCMS 2.16
  and 2.19, independent of worker count.
- GPU readback checked wide-gamut pixels, UI colors, clipping, blending and both
  sRGB fallback formats against references before the DAG rewrite; not repeated.
- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  through three window lifecycles; missing support fell back to sRGB. Physical
  display accuracy remains unmeasured.
