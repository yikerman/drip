# Progress handoff

Compact session record, newest first. Keep outcomes, evidence and unresolved
limits; implementation details and older experiments remain in Git history.
[DESIGN](DESIGN.md) holds intent; [TODO](../TODO.md) holds unfinished work.

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
