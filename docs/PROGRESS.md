# Progress

Validation evidence as of 2026-10-07. Decisions are in [DESIGN](DESIGN.md), open
work in [TODO](../TODO.md), implementation history in Git.

## Current validation

- The concrete-port rewrite passed 164 runtime/example tests, including the RAW
  corpus, plus seven compile-fail doctests. Follow-up graph, typed-port and scope
  checks passed all 22 tests, including new camera/working RGB scope parity.
- Strict workspace Clippy, formatting and five GUI binding compilation probes
  passed. `cargo doc --no-deps` now passes without warnings across the workspace
  after renaming the CLI binary to `drip-cli`; `cargo build -p drip-cli --bin
  drip-cli` also passed.
- Windows CI failed first at `2156c28`: `cb27708` had changed `gpu-allocator`'s
  locked `windows` dependency to 0.58.0 while `wgpu-hal` retained 0.62.2, breaking
  DX12 interface types. Restored their shared 0.62.2 resolution and added
  `--locked` to CI Cargo builds, lint and tests. The Windows MSVC `wgpu-hal`
  check (including DX12), Linux workspace/all-targets build and formatting pass.
  A full native Windows build remains for CI.
- Repeated the build and Windows backend checks after raising the Rust minimum
  to 1.95 and refreshing dependencies. Checks used 1.96.1; the minimum toolchain
  remains unverified.
- Validation used Linux/rustc 1.96.1. The GPU color-reference test passed before
  the DAG rewrite and was not rerun afterwards; display code was unchanged.

## Processing

- All 15 active RAWs across 13 manufacturers rendered without skips. Export
  matched full processing within one 16-bit code. Earlier visual inspection of
  14 overviews/crops found no structural corruption; some cool/cyan/blue casts
  still need measured targets. Rejected camera metadata cases remain in TODO.
- TIFF metadata matched ExifTool/exiv2; capture time was checked under UTC,
  Tokyo and Los Angeles. Proof/export parity and gamut behavior passed with
  LittleCMS 2.16 and 2.19, including worker-count invariance.
- Headless interaction tests pass. They do not establish native window behavior
  or physical display accuracy.

## Display

- GPU readback checked wide-gamut pixels, UI colors, clipping, blending and both
  sRGB fallback formats against colorimetry/LittleCMS references.
- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  through three window lifecycles. Missing support and injected setup failures
  selected sRGB and exited normally over three runs. Release GUI and Wayland
  parenting were also exercised by the user.
- NVIDIA, other compositors, Windows/macOS, HDR white, physical accuracy and
  monitor transitions remain unverified. Hidden-window pacing and native dialogs
  were not retested after their fixes.

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
