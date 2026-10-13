# Progress

Validation evidence, not a feature inventory. Open gaps are in [TODO](TODO.md).

## 2026-10-13

- Explicit compute model, step 2: the `map` shape and fused segment kernels.
  `shape.rs` holds `Shape` (now with `Map(&MapBody)`), `Pixel` and
  `MapPayload`. `plan.rs` holds `Plan` and the fused kernel, a
  `#[cube(launch)]` shell whose comptime body sequence calls each body's
  CubeCL expansion function. A map output is a deferred `Image`, extended by
  further maps and materialized once by the first consumer that needs
  samples. Exposure, Camera RGB to color RGB, Chromatic adaptation and Sigmoid
  are maps, and their per-node launches are gone. `tests/fusion.rs` compares the
  fused chain against the replaced kernels, kept there verbatim, on 369
  pixels with signed, zero, tiny, huge, infinite and NaN samples: bit
  identical on wgpu (RTX 3080) and the CPU runtime. It also checks that a
  fan-out materializes once, a kernel after two maps reads their samples,
  `evaluate` materializes a map output, a plan without constants runs, and
  the template's colour tail launches once after Orientation. Measured with
  a release driver kept in `../drip-benchmark-results/2026-10-13-fusion/`:
  RCD output of `fixtures/raw/sony-ilce-7rm3.arw` bound as a device raster,
  then camera matrix, CAT16 (as shot), exposure and sigmoid evaluated to a
  `Raster<Rgb>` with the terminal device wait, `DRIP_BACKEND=wgpu`,
  `RAYON_NUM_THREADS=12`, one warm-up and five samples, three alternating
  runs per build. Before is a worktree of 3479319, after is this change.
  Medians (ranges) in ms:

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
- Explicit compute model, step 1: residency left node signatures. Ports name
  payloads (`Read<'_, Rgb, Rec2020>`); an edge carries an `Image`, today a
  `Raster` with lazy host and device representations; `Write` is materialized
  where the node writes it; `Kind::shape` is `Kernel` or `Source`; a run's
  values live in a `Store`. `Cpu`, `Device`, `Port` and `Transfers` are gone;
  the macro rejects a placement in a port type. All 17 library kinds, the GUI
  views, scopes, Perspective Transform's view and auto exposure moved
  mechanically. Transfer counts between nodes are unchanged; five tests that
  counted the final download of an `evaluate::<Cpu<_>>` result now expect one
  download fewer, since the caller reads the result. Sources hand each
  evaluation a fresh raster of their loaded host samples, so uploads are not
  retained with the node, as before. The workspace gate passes with default
  features (wgpu on the RTX 3080), with `drip/cpu`, the doctests (including a
  new `compile_fail` for a device read of `CaptureMetadata`) and the
  `selected_backend` tests; the ignored GUI scope tests pass on the GPU. No
  benchmarks were run.

## 2026-10-12

- Researched an explicit computation model for the user ([COMPUTE_MODEL](COMPUTE_MODEL.md)):
  read Drip `aec5201`, vkdt `f0be638` and darktable `ddc0bc5` (cloned),
  CubeCL 0.11.0 sources, aces-core's output transform CTL, Blender's
  realtime-compositor evaluator, Core Image, OCIO, Halide, Burn fusion,
  libvips/GEGL, Nuke Blink, Salsa and the functional-array literature.
  Counted darktable's duplicated CPU/OpenCL bodies (77 of 97 modules) and
  per-module scale handling (58 modules). Conclusion recorded as Open:
  declare node shapes (map, stencil, warp, reduce, meta, opaque), fuse
  segments between materialization points, derive regions and scale from
  shapes, add a content-addressed cache with durability and a disk tier.
  Documentation only; no code, benchmarks or dependencies changed.
- 2026-10-13: added a concrete proposal for Drip to the same note: shapes
  on `#[node]`, `Image`/`Raster`/`Store`, foreign nodes with a disk tier,
  and an eight-step migration inside the `drip` crate. Open.
- Decided with the user: Perspective Transform is parameterized by one
  four-point quad plus aspect, in bijection with projective maps, with crop
  and rotation as quads; auto-crop is a GUI constraint plus Fit; points outside
  the source are black, edge taps repeat the edge; Sample defaults to
  Lanczos-3; the transform's editing view is separate from Preview. Checked
  darktable `e43fbf3` `ashift.c` for its auto-crop modes and simplex fit.
  Documentation only.
- Built-in templates are now `templates/*.drip-template` files embedded in the
  GUI. The RAW pipeline moved to `templates/neutral-restore.drip-template`,
  generated from the previous Rust builder, so layout and template inputs are
  unchanged; the core keeps its RAW chain only as a test fixture. New creates
  Neutral restore; New from template lists built-ins. Tests check each file
  equals its re-serialization and that a template choice waits for the
  unsaved-edits prompt. All 180 workspace tests and three doctests pass with
  `DRIP_BACKEND=cpu` and `drip/cpu`; strict Clippy and formatting pass. No
  desktop session exercised. The continuous-geometry template waits for its
  nodes.
- Added Mitchell–Netravali and Lanczos-3 as shared `#[cube]` filter kernels
  (`node/filter.rs`), not yet used by a node. The outer Mitchell piece is
  factored around its double root to avoid f32 cancellation; Lanczos uses a
  range-reduced Taylor `sin(πx)` because WGSL bounds `sin` only on `[−π, π]`.
  Host results match f64 evaluations of the published formulas within 2e-7
  and 3e-7; device results match host within 1e-6 on CPU, WGPU and Vulkan
  backends. All 178 workspace tests pass with `DRIP_BACKEND=cpu` and
  `drip/cpu`; strict Clippy and formatting pass.
- Designed geometry and resampling through continuous fields
  ([FIELD](FIELD.md)) with the user: explicit Interpolate, map-only
  `RgbField -> RgbField` nodes with per-channel maps, one footprint-scaled
  Sample, and `RgbDesc::full`. Read resampling code in darktable `e43fbf3`,
  vkdt `f0be638`, RawTherapee `5f486d3` and OpenImageIO `ee1c641`; checked
  reference DOIs against Crossref. Documentation only; no code, kernels or
  measurements. Border policy, default filter and lens coordinates stay Open.
- Replaced the `encoding` tag with `io`, carried by the RAW reader and both
  exports, renamed tag `raw` to `bayer-raw` and named the reader Bayer RAW,
  leaving room for X-Trans and monochrome. All 174 workspace tests pass
  with `DRIP_BACKEND=cpu` and `drip/cpu`, including the GUI picker's tag
  tooltip; strict Clippy and formatting pass. No desktop session exercised.
- Added Export JPEG with shared ICC conversion and TIFF capture-tag writing,
  quality and independent chroma subsampling. Independent decoder tests cover
  pixels, ICC/EXIF, averaged chroma, dimension limits and project/action round
  trips. Updated the picker test for multiple encoding labels. All 171 workspace
  tests pass with `DRIP_BACKEND=cpu` and `drip/cpu` (16 device tests ignored),
  including 99 GUI tests; three doctests, formatting and strict workspace Clippy
  pass. No desktop session exercised. Scope narrowed to JPEG only; Downsample
  remains deferred and Perspective Transform needs discussion.
- Researched JPEG crate APIs and read `jpeg-encoder` 0.7.1 encoding/ICC/EXIF
  source, Drip's export/profile/reduction code and vkdt's crop/resize shaders.
  Revised the proposed backlog to Perspective Transform, Downsample and JPEG
  export; recorded an implementation sequence and filtering/preview contracts.
  No dependencies or kernels changed; no runtime or quality measurements.
- Reviewed the node registry, starting template, sigmoid, orientation, RGB
  reduction and TIFF export for an ordinary still-image node backlog. Added a
  proposed implementation order and initial scopes to TODO, with rationale
  marked Tentative in DESIGN. Documentation only; algorithms and dependencies
  remain unselected, and the packaged proof-of-concept gate is unchanged.
- Compared Drip `7d3b450` with upstream vkdt `f0be638` (2026-10-12),
  reading both core and GUI implementations. Held processing-node availability
  equal when assessing extensibility: Drip's typed host/device values,
  per-output descriptions and branch-local failures versus vkdt's Vulkan
  scheduling, frame feedback, resident display images and production frontend.
  Distinguished existing behavior from prospective calibration and temporal
  workflows. Source review only; no runtime comparison or benchmarks, and no
  implementation decisions.
- Narrowed the comparison to concrete still-image workflows: simultaneous
  looks/proofs, reusable chart calibration, full-resolution adjustment and
  collection editing. Verified preview profiles are RGB-only; separated
  prospective calibration contracts from implemented checks and avoided
  assigning an architectural winner to HDR or focus stacking by node availability.

## 2026-10-11

- Palette layout: a wgpu-rendered kittest capture (temporary, not committed)
  showed the chosen `raw` chip dark, rows as boxes with wrapped first-sentence
  summaries and the keyboard choice outlined. GUI tests open it by
  right-click again; 99 GUI tests and strict Clippy pass.
- Node palette: GUI tests open it with Space and from the editor's menu entry,
  search and click Exposure under the pointer, narrow by the demosaic chip,
  choose RCD with Enter at the visible centre, and close with Escape; a
  right-click on the background opens nothing. 98 GUI tests and strict
  Clippy pass; not exercised in a desktop session.
- Canvas wheel zoom under overlays: a GUI test scrolls over the canvas with a
  modal open and then without; it failed before the fix (the modal's wheel
  zoomed the canvas) and passes after. 98 GUI tests and strict Clippy pass.
- Node picker: GUI tests open it by right-click and by Space, search and click
  Exposure, narrow by the demosaic chip, choose RCD with Enter, check nodes
  land where the picker opened, and close with Escape; unit tests cover
  ranking and tag filters. 97 GUI tests and strict workspace Clippy pass; not
  exercised in a desktop session.
- Tags replace categories: macro tests cover `tags = [...]` and reject a
  non-list; the interim Add node menu lists tags in registry order and adds
  Exposure from `look`. Workspace tests and strict workspace Clippy pass.
- Node IDs by operation alone: workspace tests, including project round trips
  and GUI help and error labels, pass with the new IDs; strict workspace Clippy
  passes.
- Explicit GUI bindings: GUI tests for prepared views, custom controls, the
  preparation probe and the write action run through the listed bindings; a
  test checks entries are unique and never double a node action. 94 GUI tests,
  workspace tests and strict workspace Clippy pass.
- Explicit node registry: project round-trip, admission and GUI tests load
  through `Registry`; a GUI test reopens a project holding a test-only kind
  from the test list. Workspace tests and strict workspace Clippy pass. With
  `DRIP_BACKEND=cpu`, the same 12 GUI tests fail before and after the change.
- Undo/redo: GUI tests make a held slider drag one step that undoes and redoes,
  restore a removed node with its id, edges, name and layout while the RAW
  node stays the same `Arc`, and leave Ctrl+Z to a focused text field; history
  tests cover redo clearing and the depth cap. 93 GUI tests and strict GUI
  Clippy pass; not exercised in a desktop session.
- Project command shortcuts: a GUI test saves with Ctrl+S, ignores Ctrl+Shift+N
  and refuses New while an action runs. 87 GUI tests and strict GUI Clippy
  pass; not exercised in a desktop session.
- Unsaved-edit prompt before New and Open: a GUI test cancels, saves then
  replaces, discards, and replaces an unmodified project without asking. 88
  GUI tests and strict GUI Clippy pass; not exercised in a desktop session.
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
