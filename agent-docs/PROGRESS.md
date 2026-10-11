# Progress

Validation evidence, not a feature inventory. Open gaps are in [TODO](TODO.md).

## 2026-10-10

- Rechecked official ACES input-transform, exposure-scaling, ACEScg, output-transform,
  display-encoding and source-gamut guidance after CAT16 landed. User clarified
  that “scientific” means sound processing. No additional processing-stage blocker
  found for the agreed Bayer/SDR scope. Inspected RAW normalization, gain/matrix/CAT
  placement, sigmoid, ICC export and existing proof/export checks. Remaining release
  evidence is saved-edit/full-detail export acceptance and clean-desktop delivery;
  camera accuracy and absolute exposure calibration remain stated limitations.
  Documentation review only; no processing changes or new tests.
- User ran the release GUI with Vulkan on an RTX 3080. The supplied log shows
  successful CAT16 evaluation and previews after RAW selection, with a clean
  exit. Final visible previews took about 75 ms; node timings measure host
  submission, not GPU execution. The log does not establish visual color accuracy
  or saved-project/export agreement.
- Added full CAT16 scene adaptation with As shot and custom source xy controls,
  using the existing CubeCL matrix kernel. RAW keeps as-shot conditioning through
  highlights/demosaic and folds reference/as-shot correction into camera conversion.
  Both templates place adaptation before rendering. Existing graphs need the new
  node or a fresh template; no graph migration or metadata layout change.
  Colour 0.4.7 numerical references pass on CPU and WGPU, including signed/HDR
  samples. Synthetic RAW neutrality, clipping levels, generated controls and saved
  overrides pass. The ideal-XYZ-camera test independently checks cancellation of
  conditioning gains. Full CPU workspace tests and strict Clippy pass; the WGPU
  CLI exports the Nikon D70s fixture to TIFF. Formatting and whitespace checks pass.
  Interactive photo acceptance remains with the user;
  temperature/tint and neutral sampling remain follow-ups.
- Reviewed CAT16, Bradford, generalized/two-step adaptation, WGM and recent
  2025–2026 research for scene WB. Recommend full CAT16 adaptation for the first
  module; retain partial-adaptation policy as open. Recorded the distinction
  between response matrix, adaptation degree and illuminant estimation, and
  high-chroma limitations. Research only; no processing changes or tests.
- Rechecked the official ACES input, exposure-scaling, rendering, display-encoding
  and gamut guidance after device auto exposure landed. Editable scene-illuminant
  WB remains the only required processing addition for the agreed Bayer/SDR
  prototype. Orientation and auto exposure await user integration acceptance;
  packaged delivery still needs clean-system checks. Inspected current template,
  calibration, highlights, metering and TIFF paths; no new tests or GUI run.
- Moved auto-exposure metering to CubeCL. Evaluation retains device RGB;
  successive reductions produce one mean-luminance f32 for readback. Added
  reference checks across partial reduction blocks and large HDR inputs.
  All four exposure tests pass on CPU and WGPU, including fixed-point and
  button checks. Strict workspace Clippy, formatting and whitespace checks pass.
- Added Auto exposure locally in the Exposure parameter panel. It evaluates
  through Exposure at full detail, meters mean linear luminance and edits EV.
  Documented why retaining the current EV reaches the same fixed point as
  resetting to zero. The one-shot evaluation is synchronous; processing nodes
  and the worker action interface are unchanged. GUI tests pass on CPU (83,
  seven ignored); the three metering/button tests also pass on WGPU. Strict
  workspace Clippy, formatting and whitespace checks pass. Interactive testing
  remains with the user.
- Reverted the unfinished auto-exposure button and action-result changes at the
  user's request. Exposure remains manual. Retained the completed orientation,
  unified RAW metadata/calibration and ACES review for commit.
- Rechecked official ACES input-transform, ACEScg, reference-gamut-compression,
  output-rendering and display-encoding docs against the current implementation.
  Scene-illuminant WB remains the required processing feature; orientation and
  unified calibration are implemented, with user integration still pending.
  Sigmoid plus ICC output remains the accepted prototype rendering path.
  Distinguished manual exposure from calibrated scene exposure, and retained
  packaging/platform checks as separate delivery gaps. Updated DESIGN/TODO;
  source review only, no new tests, GUI run or hosted CI verification.
- Consolidated RAW calibration and common capture/lens/GPS/sensor metadata into
  one owned output. Bayer site gains now updates samples and clipping levels
  together; camera conversion, orientation and TIFF read the shared metadata.
  Existing prototype graphs using removed ports/nodes require the new template.
- Replaced the provisional bincode codec with validated rkyv 0.8 archives after
  checking its unmaintained status. Tests cover malformed buffers, exact floating
  bits, native and derived calibration, missing gains and coordinate mismatches.
  Sony lens/focal metadata matched ExifTool; geometry and common fields pass the
  decoder fixture checks. No private maker-note structures or correction profiles
  were added; TIFF's exported field subset is unchanged.
- CPU workspace checks passed (142 tests, 15 device tests ignored), plus four
  doctests. Five selected-backend WGPU checks passed, including combined gains,
  metadata transport, orientation and existing algorithm references. Strict
  workspace Clippy, formatting and whitespace checks passed. Interactive
  integration remains assigned to the user; no GUI session was exercised here.

- Audited capture metadata versus TIFF output for future lens correction. RAW
  exposes camera make/model, ISO, shutter, aperture, focal length, timestamp,
  capture-time text and orientation; TIFF writes the descriptive subset and ICC
  profile, omitting original orientation. LibRaw lens fields are not copied by
  the shim. Recorded missing lens identification, crop-factor/profile resolution
  and focus-distance/geometry needs against the Lensfun API. Source review only;
  no processing changes or tests.
- Implemented metadata-driven orientation with the requested compute subagent.
  RAW decoding now carries LibRaw orientation as EXIF 1–8. The Geometry node
  permutes RGB samples with one CubeCL kernel; its generated dropdown replaces
  metadata with an explicit transform. The core/GUI starting template places it
  after camera-color conversion, shared by views and export. TIFF retains the
  evaluated pixel order without copying capture orientation.
  CPU-backed workspace checks pass: 138 tests, 14 opt-in ignored, and four
  doctests. Orientation permutations also pass explicitly on WGPU. Coverage
  includes all eight transforms, single-row/column images, metadata transport,
  missing metadata, overrides, dropdown interaction, project reopening and
  omission of the TIFF orientation tag. Strict workspace Clippy, formatting and
  whitespace checks pass. Interactive integration testing is left to the user.
- User refined prototype scope: require scene-illuminant adaptation and
  orientation; accept LittleCMS/rendering intent and unavailable camera
  calibration as an input limitation. Compared darktable's CAT16 workflow with
  CAM16/Hellwig appearance modeling. Proposed separate camera-reference gains
  and scene adaptation, plus shared RGB orientation; implementation remains open.
  Kept known DNG extraction failures distinct from absent matrices. Documentation
  only; no processing changes or test run. Whitespace check passed.
- Reviewed official ACES input, output, ACEScg and gamut-compression guidance
  against the current RAW template, normalization/matrix, sigmoid, preview,
  TIFF, save and distribution paths. The main processing stages exist; editable
  WB, orientation and target-aware gamut rendering remain follow-ups. Packaging,
  clean-desktop RAW-to-TIFF use and declared platform support remain the accepted
  proof-of-concept gates. Recorded proposals without changing that milestone.
  Source/documentation review only: no new image measurements, GUI exercise,
  hosted CI verification or test run.
- Diagnosed CI run 38046592913: CPU-backed GUI evaluation timed out on all
  three platforms after scope migration. Enabling `drip/cpu` alone does not
  select that runtime for GUI tests; CI explicitly sets `DRIP_BACKEND=cpu`.
  Exposure scopes forced 256 barrier lanes, while the CPU runtime needs a host
  worker per lane. Capped lanes at the reported limit and distributed the bins
  across them. Added a small-workgroup histogram/waveform regression.
  Worker evaluation waits now share the GUI's 90-second bound because the
  compute client queues requests from concurrent full-RAW tests; synchronization
  gates retain their five-second bound. No test was disabled.
  The exact CPU-backed workspace command passes locally: 132 passed, 13 ignored,
  including a two-core affinity run. All six scope tests pass explicitly on CPU,
  Vulkan and WGPU; the small-workgroup regression also passes on Vulkan. Four
  doctests, strict workspace Clippy, formatting and whitespace checks pass.
  Hosted Windows/macOS validation remains pending the follow-up commit.
- Applied the approved frontend naming, parameter, template and presentation
  changes; refreshed current help, READMEs and maintained design notes. Typed
  choice variants round-trip through Serde and show only their own settings,
  retaining inactive edits in temporary UI state. Scope overlays keep axes and
  compact color-space labels, without counts or repeated configuration.
- CPU-backed workspace tests passed (138 passed, seven ignored), including
  nested-choice editing, imported node naming and untitled-template save/title
  regressions. Strict workspace Clippy, formatting and whitespace checks passed.
  Native dialogs and visual layout were not exercised interactively. Persisted
  IDs and changed parameter layouts intentionally have no migration aliases.
- Moved histogram, waveform and vectorscope preparation (including camera
  exposure scopes) to GUI-owned CubeCL kernels consuming device RGB. LittleCMS
  proofing and CPU preview packing are unchanged. GPU scope regressions pass on
  Vulkan and WGPU; exposure counts match the CPU reference exactly. One of
  262,144 vectorscope test samples changes bins versus f64; counts, neutrals,
  primaries and HDR cases pass. GUI suite: 77 passed, 7 opt-in ignored; six scope
  tests also pass explicitly on both GPU paths. Strict GUI Clippy and formatting
  pass. CPU backend tests were skipped at the user's request.
- Preserved the original profile and reran after the user confirmed competing
  compilation had finished; excluded the earlier contaminated attempt. Clean
  WGPU baseline→device scopes: quarter 83.3→44.5 ms, half 187.5→68.0 ms,
  full 597.4→164.0 ms. Vulkan: 82.6→45.4, 195.0→67.8, 571.9→170.6 ms.
  Each case has five samples; desktop activity/clocks remain uncontrolled.
  Separate full-detail tracing measures scopes at 9.4 ms, packing at 63.7 ms,
  and GPU completion/shared readback at 85.8 ms. CubeCL scope kernels total
  6.5–6.9 ms; all kernels total 28.9 ms. Nsight CPU samples now concentrate on
  preview packing. Preview bytes match both backends and the saved baseline.
  Source, binaries, raw results, report and profiler captures are preserved in
  `../drip-benchmark-results/2026-10-10-device-scopes/`. No benchmark hooks were
  added to production code; these worker timings exclude drawing/texture upload.
- Ran the uniform Sony worker benchmark after scratch/packing changes, with
  release builds and 12 Rayon threads. Vulkan quarter/half/full medians were
  83.0/184.1/552.9 ms; WGPU 81.5/190.0/552.1 ms (five warm samples each).
  Separate host tracing measured full-detail scope preparation at about 376 ms
  and packing at 80 ms. CubeCL kernel timings totaled 22–23 ms, including
  10.5–11.7 ms for RCD; Nsight CPU samples confirmed the scope hotspot.
  Saved source/binary identities, raw samples and CPU/Vulkan capture under
  `../drip-benchmark-results/2026-10-10-profile/`. Preview bytes matched between
  backends and the prior simplified baseline. Drawing/upload remain excluded;
  profiler runs are separate and do not isolate exact readback copy cost.
- Parallelized preview RGB f32 to RGBA f16 packing with Rayon over disjoint
  output slices, preserving pixel order and conversion. Existing packed-byte
  and image-cache ownership regression passed; GUI strict Clippy passed.
  User-provided Vulkan traces show half-resolution preparation at 7–23 ms,
  previously 74–80 ms. Interactions were not controlled; full-resolution
  preparation was 62–85 ms. Formatting and whitespace checks passed.
- Audited frontend naming and display paths against node metadata, parameter
  schemas and worker results. Found exposed persistence keys, ambiguous operation
  names, inconsistent port/state presentation, contradictory Bayer reduction
  help and scope/curve reference differences. Source inspection only; no GUI
  changes, visual run or tests. Naming choices remain open.
- Compared user-provided interactive Vulkan/WGPU traces: visible warm half-detail
  updates overlap around 175–202 ms; first loaded previews were 321.8/540.6 ms.
  Different interactions and truncated logs prevent a controlled comparison.
  Backend selection uses direct SPIR-V versus WGSL through the same wgpu runtime;
  node host timings include submission and readback waits, not isolated GPU work.
- Traced preparation costs in those logs: Vulkan generation 65 spent 144.9 ms
  of 174.6 ms preparing preview/scopes on the CPU. Before parallel packing,
  preview converted RGB f32 to RGBA f16 serially; scopes scanned separately
  with Rayon. Its 29.2 ms
  preview evaluation includes upstream GPU completion and shared RGB readback.
- Shortened RCD scratch ownership to the passes that use each buffer, without
  changing kernels or adding waits. Peak Drip-owned scratch falls from eleven
  to eight f32 lanes per pixel; physical allocation savings remain unmeasured.
  Existing CPU and WGPU algorithm tests passed, including RCD upstream vectors
  for all four Bayer phases, small images and context-driven reduction.
  Library strict Clippy, workspace formatting and whitespace checks passed.
- Audited recorded memory results against master and current RCD/evaluator code.
  The latest uniform WGPU run reduced peak RSS from 3240.5 to 2297.0 MiB;
  sampled whole-GPU maximum rose from 1342 to 4325 MiB. Before lifetime changes,
  full-detail RCD requested eleven f32 scratch lanes per pixel (1.74 GiB for Sony),
  whereas master uses bounded tile scratch. Pool reservations, transfer storage
  and live buffers were not separately measured; no new benchmark was run.
- Blocked exports during asset reloads in the main and popped-out windows and
  guarded action submission against same-frame bindings. Regression checks cover
  invalidation, re-enabling after commit and replacement parameters.
- CPU-backed GUI tests passed (76 passed, one GPU-only test ignored), including
  both new regressions. GUI strict Clippy and workspace formatting checks passed.

## 2026-10-08

- Linux CI run 37893323997 failed in nine GUI tests after CubeCL device-service
  initialization disconnected. Replaced the temporary GUI test ignores and
  distribution failure bypass with CPU-backed CI tests. Clippy now explicitly
  excludes dependency linting; doctests already select workspace packages only.
  Distribution jobs only build and upload artifacts.
- CPU-backed workspace tests and four doctests passed, including all ten restored
  GUI tests (74 passed, one GPU display test ignored). Workspace-only strict
  Clippy, formatting and workflow lint passed; runner-label warnings were
  suppressed for the older local actionlint. Hosted cross-platform runs remain
  unverified.

## 2026-10-07

- Inspector node help now reflows source-wrapped sentences while preserving
  paragraphs. The updated inspector help test and workspace formatting check
  passed.
- Compacted agent documentation to retain rationale, validation evidence and
  unfinished work; removed implementation summaries and redundant history.
- After the dependency-refresh rebase, Linux workspace tests passed (165 passed,
  one GPU-only test ignored), as did strict Clippy. Earlier distribution tests
  passed (170 including doctests, one ignored). Rust 1.95 remains unverified.
- Hosted Linux tests passed in [run 37713841713](https://github.com/yikerman/drip/actions/runs/37713841713).
  Windows/macOS and distribution outcomes were not recorded in this session.
- RAW decoding/error tests passed with both LF and a temporary CRLF source
  manifest. Upstream GNU/MSVC manifests enumerate the same 79 translation units.
- Linux staging was checked with explicit and default targets: both executable
  contents/modes matched, stale files disappeared, and failed builds preserved
  the previous output. Actionlint passed with outdated runner-label checks
  suppressed. Native Windows/macOS staging remains unverified here.

## Image and display evidence

- All 15 active RAWs across 13 manufacturers rendered without skips. Export
  matched full processing within one 16-bit code. Earlier visual inspection of
  14 overviews/crops found no structural corruption; cool/cyan/blue casts still
  need measured targets. Export parity does not establish colorimetric accuracy.
- TIFF metadata matched ExifTool/exiv2; capture time was checked under UTC,
  Tokyo and Los Angeles. Proof/export parity and gamut behavior passed with
  LittleCMS 2.16 and 2.19, including worker-count invariance.
- GPU readback checked wide-gamut pixels, UI colors, clipping, blending and both
  sRGB fallback formats against colorimetry/LittleCMS references. This preceded
  the DAG rewrite and was not repeated afterwards.
- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  through three window lifecycles. Missing support and injected setup failures
  selected sRGB and exited normally over three runs. The user exercised release
  GUI and Wayland parenting. Physical display accuracy remains unmeasured.

## Historical measurements

These explain choices, not current latency guarantees.

- 2026-10-04, Sony fixture, Ryzen 5600G/12 workers, RTX 3080: Rayon 162 ms;
  CubeCL CPU 582 ms/GPU 623 ms, with a retained RAW pyramid. Removing that pyramid
  reduced decoded-resource storage from about 226 MB to 85 MB.
- 2026-10-05, release RCD/highlight pipeline, seven cycles/12 workers: detail-change
  medians 1.20 s half / 2.64 s full. Includes sensor processing/cache replacement,
  excludes preparation/upload/draw; filesystem caches uncontrolled. Reproduce
  with [preview_latency](../crates/drip/examples/preview_latency.rs).
- Half detail (3984 × 2660), LittleCMS 2.16, 12 workers: parallel ICC conversion
  reduced gamutcheck 2.8–3.0 s → 0.68–0.69 s and softproof 3.5–3.6 s → 0.51–0.57 s,
  excluding upstream work/drawing. Lab gamut-check input reduced cyan coverage
  24.7% → 3.9%; that mask remains approximate.
