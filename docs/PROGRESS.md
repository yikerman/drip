# Progress

Current implementation and validation evidence. [DESIGN](DESIGN.md) owns decisions;
[TODO](../TODO.md) tracks remaining work. Earlier implementation history is in Git.

## 2026-10-07: GUI presentation cleanup

- Separated presentation values, drawables and the image cache in naming; moved
  the image drawable beside its implementation. Removed the unused outer view
  override and named parts. Prepared nodes retain a standard viewer alongside
  custom controls, including while incomplete or failed.
- Parameter panels borrow graph values instead of cloning entire nodes per frame.
  GUI state accessors centralize layout/detail keys, defaults and validation;
  edits preserve unknown JSON fields and retain the shared graph edit path.
- Validation: 71 GUI tests passed, including saved-state validation, independent
  controls/view behavior, pop-outs and worker ownership/stale-result handling.
  The GPU-only test was not rerun. All five binding compilation probes, strict
  workspace Clippy, formatting, whitespace and local documentation links passed.
  GPU allocation policy and scope-mesh rendering are unchanged.

## 2026-10-07: Declaration cleanup

- Restricted `#[node]` to functions and kernel-less signatures; migrated the P3
  extension test off the static form. Documented generated identity names and the
  wrapper syntax/alias limitation, with parser coverage for qualified paths and
  explicit alias rejection.
- Removed unused erased `NodeKind` documentation builders; typed declarations
  remain the metadata source. Explained the owner-erasing check/action accessors.
- Validation: 31 library/macro/typed-port/declaration tests and eight compile-fail
  doctests passed. Strict workspace Clippy, formatting, whitespace checks and
  local documentation links passed.

## Current implementation

- Logical interpretations over `RawMat<C>` separate storage from capabilities.
  Local macros generate trait evidence, typed node/parameter contracts, help and
  discovery. Graph edits check downstream consequences transactionally; evaluation
  checks the same requirements plus actual-value predicates and preservation
  witnesses. Existing serialized IDs, ports, keys and choices are retained.
- Library kernels return output tuples. GUI nodes own proofing, scopes, drawing
  and preparation caches. Input observation evaluates dependencies without running
  the observed kernel. Custom controls retain the standard viewer;
  computational and presentation failures remain separate.
- RAW/ICC reads use shared snapshots. Worker coalescing, generation checks and
  image-allocation ownership keep obsolete work and deallocation off the UI path.
  Export retains its captured graph/resources across edits and resets.

## Validation baseline: 2026-10-07

- Before the cleanup above: 173 workspace tests passed, including eight compile-
  fail doctests; the normally ignored GPU color-reference test passed separately.
  Strict workspace Clippy, formatting and whitespace checks passed.
- The GUI binding harness passes a valid control and rejects wrong declaration
  identity (even with matching parameters/inputs), input signature, presentation
  type and private construction. Error codes and primary locations are checked.
  It runs from `/tmp`, cleans temporary sources and reuses repository `target/`.
- Warning-free Rustdoc passed excluding `drip-cli`: its `drip` binary collides
  with the library documentation output path in whole-workspace builds.
- Validation is on Linux/rustc 1.96.1. Rust 1.92 and Windows/macOS execution remain
  untested. Signature checks do not prove mathematical laws or numerical kernels.

## Processing and interaction evidence

- The active corpus has 15 images across 13 manufacturers, including Nikon Z6 II
  and the original Sony photograph; all rendered without skips. Corpus tests cover
  metadata/CFA, full and reduced results, finite pixels, TIFF dimensions/metadata,
  ICC bytes and export pixels against full processing within one 16-bit code.
  Unusable Canon D30, Samsung GX-1L and Sigma fp samples are tracked in TODO.
- Earlier visual review found no obvious structural corruption in 14 overviews
  and center crops. Cool/cyan/blue casts in some samples require measured targets;
  plausible renderings and export parity are not colorimetric ground truth.
  To retain review TIFFs:
  `DRIP_RAW_REVIEW_DIR=/tmp/drip-pixls-review cargo test -p drip --test pixls --release -- --nocapture`.
- TIFF capture metadata matched ExifTool/exiv2 readback; capture time was checked
  under UTC, Tokyo and Los Angeles. Proof/export parity and gamut behavior were
  tested with LittleCMS 2.16 and 2.19, including worker-count invariance.
- Headless GUI tests cover editing gestures, reset/wheel behavior, pop-out geometry,
  pointer-leave scrolling, coalescing, resource invalidation, export isolation and
  worker failure notification. They do not establish native interaction quality.
  A user-reported native teardown fault after panic remains in TODO.

## Display evidence and limits

- GPU readback checked wide-gamut image values, UI colors, clipping, out-of-volume
  blending and both sRGB fallback formats against colorimetry/LittleCMS references.
- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  through three window lifecycles. Missing target support and injected setup
  failures each selected sRGB and exited normally over three runs. The user also
  exercised the release GUI and Wayland parenting.
- NVIDIA, other compositors, Windows/macOS, HDR desktop-white handling, physical
  accuracy and cross-monitor transitions remain unverified. Hidden-window pacing
  and native dialogs were not retested after their fixes. Screenshots do not
  establish physical output accuracy.

## Performance evidence

Historical measurements explain choices, not current latency guarantees. On the
Sony fixture, Ryzen 5600G/12 workers and RTX 3080, the 2026-10-04 trial measured
Rayon at 162 ms versus CubeCL CPU 582 ms/GPU 623 ms with a retained RAW pyramid.
Removing that pyramid reduced decoded-resource storage from about 226 MB to 85 MB,
excluding results and temporaries.

The 2026-10-05 RCD/highlight pipeline measured release detail-change medians of
1.20 s at half and 2.64 s at full over seven cycles/12 workers. These include sensor
processing/cache replacement, exclude GUI preparation/upload/draw, and have
uncontrolled filesystem caches. Exposure/sigmoid edits reuse upstream results.
Reproduce with [preview_latency](../crates/drip/examples/preview_latency.rs).

At half detail (3984 × 2660, LittleCMS 2.16, 12 workers), parallel ICC conversion
reduced gamutcheck from 2.8–3.0 s to 0.68–0.69 s and softproof from 3.5–3.6 s to
0.51–0.57 s, excluding upstream processing/presentation. Lab gamut-check input
reduced cyan coverage from 24.7% to 3.9%; the sampled boundary remains approximate.
