# Progress

Validation evidence and milestones, not a feature inventory. Open gaps are
in [TODO](TODO.md).

## 2026-10-13

- Explicit compute model, step 2: the map shape and fused segment kernels.
  Exposure, Camera RGB to color RGB, Chromatic adaptation and Sigmoid are
  maps. `tests/fusion.rs` compares the fused chain against the replaced
  kernels, kept there verbatim, on 369 pixels with signed, zero, tiny, huge,
  infinite and NaN samples: bit identical on wgpu (RTX 3080) and the CPU
  runtime. It also checks that a fan-out materializes once, a kernel after
  two maps reads their samples, `evaluate` materializes a map output, a plan
  without constants runs, and the template's colour tail launches once after
  Orientation. Measured with a release driver kept in
  `../drip-benchmark-results/2026-10-13-fusion/`: RCD output of
  `fixtures/raw/sony-ilce-7rm3.arw` bound as a device raster, then camera
  matrix, CAT16 (as shot), exposure and sigmoid evaluated to a `Raster<Rgb>`
  with the terminal device wait, `DRIP_BACKEND=wgpu`, `RAYON_NUM_THREADS=12`,
  one warm-up and five samples, three alternating runs per build. Before is a
  worktree of 3479319, after is 17142d6. Medians (ranges) in ms:

  | Scenario | Scale | Before | After |
  |---|---|---|---|
  | Colour tail | 1 | 6.60 (6.38–17.11) | 1.82 (1.64–2.09) |
  | Colour tail | 2 | 1.80 (1.67–2.05) | 0.62 (0.58–0.69) |
  | RAW to sigmoid, RAW decoded once | 1 | 50.37 (49.15–62.02) | 46.78 (45.89–58.41) |
  | RAW to sigmoid, RAW decoded once | 2 | 36.54 (35.97–37.64) | 35.59 (34.94–36.07) |

  Output hashes agree before and after in every scenario. Build each variant
  with `CC=/usr/bin/cc CXX=/usr/bin/c++ CARGO_TARGET_DIR=$PWD/target cargo
  build --release --offline` in its `before/` or `after/` directory and run
  `DRIP_BACKEND=wgpu RAYON_NUM_THREADS=12 DRIP_BENCH_SAMPLES=5
  DRIP_BENCH_RAW=<fixture> ./target/release/fusion-bench`. The full
  workspace gate passes.
- Explicit compute model, step 1: residency left node signatures. All 17
  library kinds, the GUI views, scopes, Perspective Transform's view and auto
  exposure moved mechanically. Transfer counts between nodes are unchanged.
  Five tests that counted the final download of an `evaluate::<Cpu<_>>`
  result now expect one download fewer, since the caller reads the result.
  The workspace gate passes with default features (wgpu on the RTX 3080),
  with `drip/cpu`, the doctests (including a `compile_fail` for a device read
  of `CaptureMetadata`) and the `selected_backend` tests. The ignored GUI
  scope tests pass on the GPU. No benchmarks were run.

## 2026-10-12

- Perspective Transform and its editing view. An integer crop through Sample
  reproduces its pixels exactly, and a keystone matches an independent f64
  resampler on CPU, WGPU and Vulkan. GUI tests cover the view's preparation on
  the compute runtime and its frame geometry. Not exercised in a desktop
  session.
- `RgbField`, Interpolate and Sample: interpolating filters reproduce their
  input exactly at factor one, signed zeros included. Tests cover constants
  under rotation, shear and scale, 2×2 box averaging, Nyquist antialiasing
  and preview framing, and per-channel projective maps match an independent
  f64 resampler on CPU, WGPU and Vulkan.
- Mitchell–Netravali and Lanczos-3 filter kernels (`node/filter.rs`): host
  results match f64 evaluations of the published formulas within 2e-7 and
  3e-7, and device results match host within 1e-6 on CPU, WGPU and Vulkan.
- Built-in templates moved to `templates/*.drip-template`, generated from the
  previous Rust builder, so layout and template inputs are unchanged. Tests
  check each file equals its re-serialization. All 180 workspace tests and
  three doctests pass with `DRIP_BACKEND=cpu` and `drip/cpu`.
- Export JPEG: independent decoder tests cover pixels, ICC/EXIF, averaged
  chroma, dimension limits and project/action round trips. All 171 workspace
  tests pass with `DRIP_BACKEND=cpu` and `drip/cpu`.

## 2026-10-11

- GUI editing: GUI tests cover the node palette (Space, menu entry,
  right-click, search, tag chips, Enter and Escape), undo/redo, project
  shortcuts and the unsaved-edits prompt. A wheel-zoom test failed before its
  fix (a modal's wheel zoomed the canvas) and passes after. A wgpu-rendered
  capture of the palette was inspected. Not exercised in a desktop session.
- Lazy evaluation. Connecting Demosaic to Orientation in the template had been
  accepted without a RAW and refused with one. A regression test now wires and
  selects the RAW in both orders and reopens the project. Tests show a RAW
  decoded once and shared by graph snapshots after its file is deleted, and an
  ICC profile read at first use.
- CI run 38164895447 timed out on macOS waiting for a dropped worker's preview:
  the five-second wait shared the CPU compute client with full-RAW tests.
  Compute completions now get the 90-second allowance. macOS is unverified.
- Parallel GUI device tests crashed (SIGSEGV) in concurrent Vulkan extension
  enumeration: a two-thread C probe crashes 5/5 with only the NVIDIA ICD and
  passes 5/5 with lavapipe. Initializing the compute runtime before the
  display test's wgpu instance fixes it: all seven GUI device tests pass in
  parallel on WGPU, including five fresh-process reruns, and on CPU.
- Desktop check: `dist/drip-gui` on KDE Wayland moved, resized and opened
  Preview detail through emulated pointer input.
- Model refactor: typed payloads and port requirements, typed defaults, static
  kinds, RAW as a source, TIFF export as a node action, one worker queue and
  per-project workers. The CLI exported the Nikon D70s fixture, and auto
  exposure ran twice end to end on the Sony fixture.

## 2026-10-10

- u8 TIFF export: all depths, uncompressed/Deflate output, exact embedded ICC
  bytes and sRGB-encoded neutrals pass, as does proof/export parity.
- CAT16: Colour 0.4.7 references pass on CPU and WGPU, including signed/HDR
  samples. Synthetic RAW neutrality, clipping levels and ideal-XYZ-camera gain
  cancellation pass. The user's release Vulkan run on an RTX 3080 evaluated
  CAT16 and previews cleanly. It does not establish color accuracy or
  saved-project/export agreement.
- Device auto exposure: reference checks across partial reduction blocks and
  large HDR inputs pass on CPU and WGPU.
- RAW metadata and orientation: Sony lens and focal metadata match ExifTool.
  All eight orientations pass on WGPU, including single-row/column images and
  the omitted TIFF orientation tag.
- CI run 38046592913 timed out on CPU-backed GUI tests: fixed 256-lane barrier
  groups oversubscribe the CPU runtime. Capped lanes at the reported limit.
  Tests pass in a two-core affinity run, and scope tests pass on CPU, Vulkan
  and WGPU.
- Device scopes: exposure counts match the CPU reference exactly. One of
  262,144 vectorscope samples changes bin versus f64. Clean WGPU baseline →
  device scopes: quarter 83.3→44.5 ms, half 187.5→68.0 ms, full 597.4→164.0 ms.
  Vulkan 82.6→45.4, 195.0→67.8, 571.9→170.6 ms (five samples each). Full-detail
  tracing: scopes 9.4 ms, packing 63.7 ms, GPU completion/readback 85.8 ms.
  Preview bytes match both backends and the baseline. Artifacts:
  `../drip-benchmark-results/2026-10-10-device-scopes/`.
- Parallel preview packing reduced half-detail preparation from 74–80 ms to
  7–23 ms in user Vulkan traces (uncontrolled interactions).
- RCD scratch lifetimes: peak Drip-owned scratch falls from eleven to eight f32
  lanes per pixel. RCD upstream vectors pass for all four phases on CPU and
  WGPU. The latest uniform WGPU run had peak RSS 2297.0 MiB (was 3240.5).
  Sampled whole-GPU maximum rose from 1342 to 4325 MiB. Live allocations
  unmeasured.

## 2026-10-08 and earlier

- The CubeCL rewrite passes the workspace suite on the real GPU and the CPU
  backend. Opposed highlights match the f64 reference within 9.54e-7 (CPU) and
  1.55e-6 (WGPU) and pinned upstream vectors within 2e-5. RGB reduction
  matches an f64 reference over partial blocks, HDR and nonfinite values.
- 2026-10-07: hosted Linux
  [run 37713841713](https://github.com/yikerman/drip/actions/runs/37713841713)
  passed after the dependency refresh. RAW decoding passes with LF and CRLF
  source manifests, and upstream GNU/MSVC manifests list the same 79
  translation units. Rust 1.95 and Windows/macOS staging remain unverified.

## Image and display evidence

- All 15 active RAWs across 13 manufacturers rendered without skips. Export
  matched full processing within one 16-bit code. Visual inspection found no
  structural corruption. Cool/cyan/blue casts still need measured targets.
- TIFF metadata matched ExifTool/exiv2, with capture time checked under three
  time zones. Proof/export parity and gamut behavior passed with LittleCMS 2.16
  and 2.19, independent of worker count.
- GPU readback checked wide-gamut pixels, UI colors, clipping, blending and both
  sRGB fallback formats against references before the DAG rewrite. Not
  repeated.
- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  through three window lifecycles. Missing support fell back to sRGB. Physical
  display accuracy remains unmeasured.
