# Progress

Validation evidence, not a feature inventory. Open gaps are in [TODO](TODO.md).

## 2026-10-11

- Compacted the agent docs and removed entries the lazy model made stale or
  misleading, in DESIGN, PROGRESS, TODO, TYPE_SYSTEM, the README and three
  code comments. Workspace tests pass; documentation only.
- Lazy evaluation. Graph edits check ports only: connecting Demosaic to
  Orientation in the template had been accepted without a RAW and refused with
  one, and selecting a RAW into that wiring was refused with an unattributed
  Sigmoid requirement error; a regression test now wires and selects the RAW in
  both orders and reopens the project. Missing inputs are reported by the
  incomplete node. Files are read when a node is first forced and cached in the
  node: tests show a RAW decoded once and shared by graph snapshots after its
  file is deleted, and an ICC profile read at first use. GUI previews force only
  nodes with a view; a failed ancestor shows its own error and views name it.
  Workspace tests, including 86 GUI tests, and strict workspace Clippy pass.
- CI run 38164895447 timed out on macOS waiting for a dropped worker's preview:
  the wait from `31f2c18` allowed five seconds while sharing the CPU compute
  client with full-RAW tests. Compute completions now get the 90-second
  evaluation allowance. Linux CPU GUI tests pass; macOS is unverified.
- Parallel GUI device tests crashed (SIGSEGV) in concurrent Vulkan extension
  enumeration: a two-thread C probe crashes 5/5 with only the NVIDIA ICD and
  passes 5/5 with lavapipe. `4e44ccd` added compute scopes to the ignored GUI
  tests, which identifies the trigger by history, not a driver bisection.
  Initializing the compute runtime before the display test's wgpu instance
  fixes it: all seven GUI device tests pass in parallel on WGPU, including five
  fresh-process reruns, and on CPU.
- Mechanical simplifications: one core kind index, core port handles in the
  GUI, typed template handles, shared canvas snapshots, no retained preview
  source pixels, fewer wrappers and report fields. CPU workspace tests pass
  (159, with 16 device tests ignored), as do doctests, strict Clippy and
  formatting; the 16 device tests pass on WGPU with one test thread.
- Source review of the proposed simplifications: winit 0.30.13 returns an empty
  title on X11 (unsupported) and keeps it on Wayland; core scales accept every
  integer from 1 to 256 while GUI levels are powers of two. No tests run.
- README refreshed with the five Sony demos inline and captioned, including the
  gamut warning; images, links, LFS attributes and whitespace checked.
- CPU CI run 38128023310 lacked a `Cpu` import in the feature-gated RAW pipeline
  test; restored. CPU workspace tests (157, 16 ignored), doctests, strict
  Clippy and formatting pass.
- Desktop check: `dist/drip-gui` on KDE Wayland moved, resized and opened
  Preview detail through emulated pointer input; the captured menu was
  inspected.
- Model refactor implemented in steps, each with GPU workspace tests, Clippy
  and formatting passing: typed payloads and port requirements (one RGB
  payload, facts-only RAW metadata, merged Camera/Color nodes), typed defaults
  and file roles, static kinds with generated inputs and GUI bindings, RAW as a
  `Load` source, TIFF export as a node with an action (the CLI exported the
  Nikon D70s fixture), one worker queue for previews and actions (auto exposure
  ran twice end to end on the Sony fixture), per-project workers, and the
  project format version following the `drip` major version.

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
  metadata transport, orientation and algorithm references. Sony lens and focal
  metadata match ExifTool. All eight orientations pass on WGPU, including
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
