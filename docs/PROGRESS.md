# Progress handoff

Compact session record, newest first. Keep outcomes, evidence and unresolved
limits; implementation details and older experiments remain in Git history.
[DESIGN](DESIGN.md) holds intent; [TODO](../TODO.md) holds unfinished work.

## 2026-10-05: workflow README and demo

- Rewrote the README as a product introduction: photographic workflow first,
  followed by setup and development links. Kept color coverage concise and
  retained prototype limits; used darktable, vkdt and Capture One as tone/flow
  references without copying their prose.
- Added dependency setup for Fedora, Debian/Ubuntu, Homebrew and Windows MSVC/
  vcpkg. Checked package names and build-script discovery; macOS and Windows
  instructions remain unverified on those platforms.
- Added the user-provided workspace, full-pipeline and pop-out screenshots under
  fixtures/demos, tracked
  with Git LFS. Checked the README links and staged LFS pointer.

## 2026-10-05: scope label glyphs

- Replaced unsupported prime/arrow glyphs in scope labels with ASCII apostrophes
  and plain “image x”. User screenshot exposed missing glyphs in the UI font.
- Formatting and diff checks passed; no processing changes.

## 2026-10-05: shared scope presentation

- Histogram now shares the scopes’ black background. Consolidated clipping,
  labels, normalized coordinates, count scaling, RGB colors and EV positioning.
  Increased label and histogram trace contrast for the black plotting area.
- All 17 GUI tests, formatting and GUI clippy passed.

## 2026-10-05: default probes

- New projects include histogram, waveform and vectorscope after sigmoid, beside
  the preview. Existing saved projects retain their own graphs.
- All 17 GUI tests passed, including real RAW probe evaluation and pop-outs;
  formatting and workspace clippy passed.

## 2026-10-05: vectorscope color

- Color density bins by chromaticity, with white neutrals and brightness tied
  to count. Colors are clipped sRGB annotations; scope positions are unchanged.
- Checked neutral, primary and density colors with a targeted numerical test.

## 2026-10-05: waveform and vectorscope

- Added scope nodes using the existing resizable bodies and pop-out views;
  density counting and geometry preparation run off the UI thread.
- CIE u′v′ math and darktable behavioral references are credited in THIRD_PARTY.
  Synthetic checks cover exposure, spatial bins, channels and chromaticity.
- Workspace tests passed, followed by targeted reduction/graph/pop-out checks;
  formatting and workspace clippy passed. Native scope rendering has not been
  visually checked.

## 2026-10-05: histogram defaults and review

- Default counts to linear; logarithmic counts remain selectable. Existing saved
  settings are preserved. Reviewed binning and centered plotted samples within
  their EV bins. Boundary and parameter tests cover both count scales.

## 2026-10-05: README refresh

- Updated pipeline, preview placement, node organization, pop-out controls and
  release scope. Corrected template terminology and launch examples; checked
  descriptions against code and local document links.

## 2026-10-05: readable default typography

- Increased shared body/button text to 16 points and secondary text to 14,
  including canvas and histogram labels. Layout dimensions remain unchanged.

## 2026-10-05: sigmoid pop-out sizing

- Bounded the curve plot's preferred width to 320 points so content measurement
  produces a compact pop-out. Extended the parameter-window layout regression
  check to sigmoid, including its controls and plot caption.

## 2026-10-05: highlight reconstruction

- Ported Bayer inpaint opposed to Rust/Rayon with per-color saturation carried
  through white balance. The default template reconstructs at sensor resolution
  before demosaic reduces previews; bypassing highlights preserves preview scaling.
- Added pinned upstream C vectors, edge/partial-mask and thread checks, and a
  graph test showing reconstruction before averaging. The real RAW export test
  now exercises the new full-size default pipeline and embedded ICC profile.
- Workspace tests (89 total), clippy with warnings denied, and formatting pass.
  Release level-change medians: 1.20 s at 1/2 and 2.64 s at Full on the Sony
  fixture with 12 workers, excluding GUI work; details/limits are in DESIGN.
- Broader photo/camera validation remains in TODO. RCD uses bilinear outer
  borders; opposed includes partial mask cells and complete edge neighborhoods,
  unlike the pinned upstream implementation. Ports retain source citations.

## 2026-10-05: RCD and preview placement

- Ported RCD with bounded tile scratch and Rayon; the default template now
  produces full-size RGB at Full detail. Bilinear interpolation covers the outer
  ten pixels and tiny inputs; interior stages follow the pinned upstream source.
- Per user decision, preview reduction moved from RAW reading to demosaic input,
  leaving sensor processing at full detail even for small previews.
- Validation: 800 upstream C vectors across Bayer phases and tile seams; flat
  fields, measured samples, tiny/odd geometry and thread consistency. Workspace
  tests, including real RAW/GUI checks, and clippy/fmt passed.

## 2026-10-05: sigmoid and exposure

- Ported darktable's per-channel sigmoid, hue/energy correction and fixed smooth
  primaries to Rust/Rayon. Retained Drip's 0.18 grey and zero black; analytic
  coefficients and log-space evaluation avoid unstable powers. Exposure is a
  separate scene-linear node. A shared curve plot accompanies schema controls.
- Validation: workspace tests, f64 curve/slope checks, pinned upstream C hue
  vectors, extreme/negative inputs and one/multiple Rayon workers; clippy and fmt.
  Existing project schemas intentionally change without migration.

## 2026-10-05: third-party credits

- Added `THIRD_PARTY.md` for direct dependencies, native libraries and adapted
  algorithms, with source notices retained beside the ports. Rust licenses were
  checked against dependency manifests.

## 2026-10-05: node organization

- Moved kernels beside their backend nodes and split frontend editing from node
  presentation. Existing behavior and tests retained; no new processing yet.
- Validation: workspace tests and clippy across all targets, formatting.

## 2026-10-05: development version

- Set all four workspace crates and lockfile entries to `0.1.0-dev`.
  Offline Cargo metadata verified inherited versions; dependency versions did
  not change. No processing changes.

## 2026-10-05: v0.1 direction

- User set the milestone order: proven production algorithms and real-RAW
  validation, editing UI/UX, SpyderX color verification, then packaging/v0.1.
  Algorithm selection remains open; CLI expansion is not the next milestone.

## 2026-10-05: documentation compaction

- Replaced accumulated plans and superseded decisions with current intent and
  rationale. Removed code-shaped API inventories, abandoned backend surveys,
  duplicate benchmarks, completed milestones and repetitive session narration.
- Consolidated unfinished work without promoting deferred robustness or resource
  work. Kept the performance tradeoffs and verification limits needed to resume.
- Checked local document links and stale decision references; no runtime changes.

## 2026-10-05: review and GUI consolidation

- Node bodies and separate parameter/view pop-outs work on a transformed canvas.
  Wayland parenting and resize pacing were exercised live; canvas interaction was
  checked by the user. Transformed text removed the zoom-related atlas stalls.
- Review fixes: consume elapsed repaint deadlines even if no frame arrives;
  append `.drip` without replacing entered extensions; centralize graph edits and
  their redraw/evaluation effects. Presentation edits now refresh all windows
  without recomputation. Save/export robustness and resource tuning were deferred
  by the user, with remaining issues in TODO.
- Verification: the review baseline passed all 77 workspace tests, including the
  RAW fixture. After the edits, all 15 GUI tests, formatting and GUI clippy passed.
  The latest repaint and filename fixes were source-checked and tested headlessly;
  native hidden-window and file-dialog behavior was not exercised in that review.

## 2026-10-04: working prototype

- Graph engine, strict project/template persistence, LibRaw pipeline, ICC TIFF
  export and interactive GUI implemented. CLI remains a scaffold.
- Replaced the CubeCL trial with Rayon. Simplified preview to manual global
  detail and all-node evaluation; moved CPU work off the UI thread. Preview and
  export share decoded RAWs without retaining normalized pyramids. Historical
  performance evidence and its limits are retained in DESIGN.
- Core validation includes independent numerical references, Bayer phases and
  cropping, histogram boundaries, LibRaw comparisons, TIFF round-trips and
  scheduler/cache lifetime checks. The Sony fixture supplies real RAW coverage.
- KWin screenshot comparisons matched scRGB and explicitly tagged Rec.2020 for
  in-gamut patches. Screenshots cannot verify output beyond sRGB. Windows/macOS
  packaging and display behavior, other cameras, and instrument-based display
  verification remain unverified or limited; see TODO.

Future entries should record only material changes, relevant validation and
anything the next session needs that is not already in code, DESIGN or TODO.
