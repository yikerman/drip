# Progress

Validation evidence, not a feature inventory. Open gaps are in [TODO](TODO.md).

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
