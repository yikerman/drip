# Progress

Validation evidence and milestones, not a feature inventory. Open gaps are
in [TODO](TODO.md).

## 2026-10-13

All runs below use the Sony fixture (`sony-ilce-7rm3.arw`, 7968 × 5320),
`DRIP_BACKEND=wgpu` on the RTX 3080 and `RAYON_NUM_THREADS=12` unless
stated, and report medians (ranges) in ms. Drivers and data are in
`../drip-benchmark-results/2026-10-13-*/`. Each step passed the full
workspace gate.

- Built-in templates are Rust again: Neutral restore, Neutral correction and
  All, replacing 969 lines of template JSON. Each chain develops the
  Nikon D70s fixture into finite RGB at scale 8 on the CPU runtime
  (`built_in_chains_develop_a_raw`), and each built-in saves and reopens
  unchanged. A render of the editor through the test harness on the RTX
  3080 showed no overlapping nodes and no metadata wire crossing a node.
- Structure pass: one home for shapes, plan fronts, meta sub-graphs in the
  evaluator's own slots, `TargetValues`, per-call kernel counts. Production
  Rust lines (cloc, tests excluded): drip 7815 → 7891, drip-gui 5764 →
  5785, drip-macros 1360 → 1351. The gate passed after every commit, and
  on the default wgpu runtime the workspace tests (220 passed, 25 ignored)
  and the 15 `selected_backend` tests pass.
- A demand repeated after the frontend read its view's input, Source →
  Exposure → view on 64 × 48 samples, CPU runtime
  (`eval::tests::cpu_repeated_demand`). At full detail the second call
  runs only the view, the store serves Exposure's output with the host
  samples read after the first call, and nothing downloads. At scale 4 the
  second call also downloads nothing, but it reruns Exposure (two nodes,
  one fused launch of 192 samples), and the frontend's read downloads
  again. The view reads a reduction of Exposure's output that is the same
  image, and the run releases the unreduced value before the view
  materializes it, so neither key keeps the raster.
- Documentation cut to one job per level: crate indexes with a diagram and
  an example, module docs with their rules, and an overview ARCHITECTURE.
  The drip README's examples now compile as doc tests, and `cargo doc
  --no-deps` passes with `-D warnings`. Run once on the CPU runtime, the
  `no_run` examples pass.
- Migration benchmark, baseline (21fd470) to final (df6565f), GUI worker
  protocol plus a Sigmoid edit, a 100 % crop and a reopen, three rounds of
  five samples. Every saved preview is bit identical to the baseline's.
  Peak RSS is 2.1 GiB, after 4.8 GiB at steps 4 and 5.

  | Scenario | Baseline | Final |
  |---|---|---|
  | New Exposure, level 1 | 69.5 | 36.3 |
  | New Sigmoid contrast, level 1 | 70.2 | 37.4 |
  | Unchanged request, level 1 | 68.7 | 33.1 |
  | Scale change to level 2, 1, 0 | 45.8, 69.2, 170.3 | 12.7, 32.9, 76.7 |
  | Reopen | 472.6 | 168.2 |
  | First load | 525.9 | 589.5 |
  | 1600 × 1000 crop | | 7.3 |

  The first load includes the disk-tier write. Step 6 made a new EV faster
  (50.3 to 33.8) and store hits slower (25.2 to 33.9 at level 1), since
  Preview's preparation costs 27.7 ms on a hit instead of 18.0. Step 8 is 1
  to 3 ms slower than step 7 at level 1. Neither cause was isolated.
- Step 8, meta nodes (`tests/meta.rs`, CPU runtime and wgpu). Pyramid blur
  with three levels expands to three Resample and Sharpen pairs and a
  Resample back. A flat 37 × 29 image passes unchanged within 1e-5 relative
  (wgpu returned 1.699999 for 1.7 after seven stages, so 1e-6 failed). The
  same demand again runs nothing, scale 4 keeps the lattice, and a 24 × 16
  crop of a two-level pyramid over 96 × 80 equals the whole output bit for bit while
  writing less than half the samples.
- Step 8, stencils (`tests/stencil.rs`, CPU runtime and wgpu). Sharpen
  turns a delta into `δ + amount · (δ − G)` within 1e-6. A reach of 4 pixels
  reads 4 samples at scale 1 and 2 at scale 2, and 0.625 pixels counts one
  approximated launch at scale 2. Exposure after Sharpen runs in its launch
  with the bits of a materialized intermediate, and a crop equals the whole
  output bit for bit inside the image and at its corner. Template tail and
  Sharpen (radius 2, reach 5), new Exposure per sample, five samples:

  | Request | Samples written | Evaluate | With host read |
  |---|---|---|---|
  | Whole, level 1 | 21 194 880 | 2.23 (2.20–2.56) | 12.00 (11.99–25.72) |
  | Whole, full detail | 84 779 520 | 24.77 (24.70–32.98) | 122.11 (117.67–154.42) |
  | Crop, 1600 × 1000 | 3 226 100 | 1.01 (0.86–1.11) | 6.92 (2.84–7.79) |

  Without Sharpen, step 6 measured 10.67 and 0.43.
- Step 7, reductions (`tests/reduce.rs`, CPU runtime and wgpu). Counts match
  host counts for shared-memory and global-copy bodies over rasters from
  1 × 1 to 1000 × 3 with NaN, infinite and signed samples, and `fold` sums
  match f64. A bins node on the deferred tail runs no fused launch and one
  pass, and two consumers materialize the tail once. drip-gui launches no
  kernel (`grep -rn "cube(launch)" crates/drip-gui/src` is empty). Level 1,
  new Exposure per sample, five samples after two warm-ups, plot meshes
  excluded:

  | Targets | Step 6 | Step 7 |
  |---|---|---|
  | Histogram | 1.65 (1.61–1.70) | 1.56 (1.50–1.64) |
  | Waveform | 1.60 (1.57–1.76) | 2.26 (2.18–2.34) |
  | Vectorscope | 1.88 (1.86–2.11) | 1.44 (1.21–1.58) |
  | Three scopes | 3.92 (3.79–3.92) | 4.19 (4.18–4.33) |
  | Preview and three scopes | 3.91 (3.86–4.01) | 4.19 (4.06–4.31) |

  Recomputing the tail in three passes took 5.03. Five global copies took
  the waveform from 6.1 to its 2.26.
- Step 6, regions and the scale rule (`tests/regions.rs`, CPU runtime and
  wgpu). A 13 × 9 crop of six chains over a 61 × 47 source equals the whole
  output bit for bit, and a one-launch chain writes only the crop's 117
  samples. A 24 × 20 crop of the template's processing launches the tail
  once over 480 samples, and a pan reuses the stored RCD output. Export TIFF
  under a ten-row device budget writes more than three bands and the same
  bytes as one piece. Three tests changed on purpose for the new scale rule.
  New Exposure per sample, five samples after a warm-up:

  | Request | Samples | Evaluate | With host read |
  |---|---|---|---|
  | Whole, 7968 × 5320 | 42 389 760 | 10.67 (10.61–10.83) | 105.95 (105.59–109.40) |
  | 100 % crop, 1600 × 1000 | 1 600 000 | 0.43 (0.24–0.53) | 4.42 (4.08–6.61) |

- Step 5, foreign nodes and the disk tier (`tests/foreign.rs`, host
  runtime). A new evaluator reads a disk node's output instead of running
  it, damaged files are discarded and rewritten, the budget deletes the
  least recently used entry, and a deferred batch starts one job, reports
  pending, wakes and then hits. Mosaic and metadata, a new evaluator per
  sample:

  | Evaluation | Samples |
  |---|---|
  | Decode, no disk tier | 402.2, 398.1, 391.1 |
  | Decode and write to an empty disk tier | 463.1, 461.7, 461.2 |
  | Read from the disk tier | 90.8, 88.2, 88.8 |

  Writing the 170 MB entry adds about 63 ms to the first decode.
- Step 4, the store (`tests/store.rs`, CPU runtime and wgpu). A Sigmoid-only
  edit runs five nodes and one launch with two hits and no upload, and the
  unchanged demand runs nothing. Four tests changed on purpose. GUI worker
  protocol, worktree of f1e0697 before and after, three alternating runs of
  five samples:

  | Scenario | Level | Before | After |
  |---|---|---|---|
  | Scale change | 2 | 45.0 (44.5–48.3) | 9.9 (9.4–12.2) |
  | Scale change | 1 | 67.9 (67.0–70.1) | 24.3 (23.7–27.8) |
  | Scale change | 0 | 160.8 (157.6–177.3) | 82.5 (79.8–91.6) |
  | Exposure ±0.1 EV | 1 | 67.7 (66.6–69.3) | 25.5 (23.4–53.9) |
  | Exposure, new EV each | 1 | 68.7 (67.4–74.8) | 55.3 (47.8–72.4) |
  | Unchanged | 1 | 73.0 (67.7–76.4) | 24.9 (23.6–26.5) |

  Preview texels are identical before and after. With a new EV, Preview's
  preparation grows from 17 to 38 ms as it waits for the fused launch and
  downloads the 127 MB output.
- Step 3, warps and Resample. Orientation's permutations pass bit for bit
  through the graph on the CPU runtime and wgpu, and the colour tail with
  Orientation launches once instead of twice. Each composition rule has a
  test, and the resampling reference tests pass through Resample.
  Perspective Transform alone equals it followed by a factor-one Lanczos-3
  Resample, bit for bit. No benchmarks were run.
- Step 2, maps and fused kernels (`tests/fusion.rs`). The fused chain
  equals the replaced kernels bit for bit on 369 pixels with signed, zero,
  tiny, huge, infinite and NaN samples, on wgpu and the CPU runtime. RCD
  output bound as a device raster, then camera matrix, CAT16, Exposure and
  Sigmoid, worktrees of 3479319 and 17142d6, one warm-up and five samples:

  | Scenario | Scale | Before | After |
  |---|---|---|---|
  | Colour tail | 1 | 6.60 (6.38–17.11) | 1.82 (1.64–2.09) |
  | Colour tail | 2 | 1.80 (1.67–2.05) | 0.62 (0.58–0.69) |
  | RAW to sigmoid, RAW decoded once | 1 | 50.37 (49.15–62.02) | 46.78 (45.89–58.41) |
  | RAW to sigmoid, RAW decoded once | 2 | 36.54 (35.97–37.64) | 35.59 (34.94–36.07) |

  Output hashes agree before and after.
- Step 1, residency left node signatures. Transfer counts between nodes are
  unchanged, and five tests expect one download fewer, since the caller now
  reads the result. No benchmarks were run.

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
