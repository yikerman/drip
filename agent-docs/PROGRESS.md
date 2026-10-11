# Progress

Validation evidence, not a feature inventory. Open gaps are in [TODO](TODO.md).

## 2026-10-11

- Implemented typed payloads and port requirements: one RGB payload, typed
  interpretations, requirement type arguments, contracts over known
  descriptions, admission-time parameter checks, device layouts only for
  kernel-read payloads, facts-only RAW metadata without rkyv, and merged
  Camera/Color reduction, histogram and waveform nodes. Workspace tests pass on
  the GPU including device tests; strict workspace Clippy and formatting pass.
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
