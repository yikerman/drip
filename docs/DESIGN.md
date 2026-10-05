# Design intent

Keep the intent and tradeoffs that code cannot explain. APIs, schemas, defaults
and algorithms belong in code and tests; usage belongs in README. Superseded
plans and detailed experiments remain in Git history. Unfinished work lives in
[TODO](../TODO.md), and verification limits in [PROGRESS](PROGRESS.md).

Statuses: **requirement** = user goal; **decided** = agreed direction;
**tentative** = proposal; **open** = unresolved. Decisions can be reconsidered
as the prototype develops; they are not permanent architectural constraints.

## Scope and boundaries

- **Requirement:** A reusable processing library with interactive and batch
  frontends. Processing, graph validation, color management and persistence
  must work without a GUI dependency.
- **Requirement:** Linux on Wayland is primary; Windows and macOS should build
  in CI but have lower testing priority. X11 is unsupported.
- **Requirement:** Use LibRaw for decoding and LittleCMS for ICC transforms.
- **Decided:** Keep a minimal owned-data LibRaw binding. A C shim avoids
  mirroring version-dependent native struct layouts; native handles do not
  escape into graph values. Existing bindings had ownership or maintenance
  problems when surveyed.
- **Decided:** Prefer a closed set of semantic image types over plugin
  machinery. Scene and display RGB are distinct so export requires tone
  mapping. Camera characterization travels with its image to prevent mixing
  one camera/file's matrix with another image.
- **Decided:** Do not encode white-balance history in types. This keeps the
  graph flexible, including debayer-before-WB, at the cost of allowing double WB.
- **Decided:** One parameter schema serves editing, persistence and templates.
  Presentation data is headless; node-specific widgets belong in the frontend.
- **Decided:** Evaluation has no output side effects. File writes require an
  explicit action, so cache hits, preview refreshes and preview detail cannot
  accidentally control whether or how an export runs.

## Responsiveness and resource costs

- **Requirement:** Prioritize responsiveness, portability and easy development
  over peak throughput. Keep one source implementation per computational kernel
  across backends; acceleration should remain internal to nodes. Compare whole
  calls, including allocation and transfers, rather than device timing alone.
- **Decided:** Use Rayon and postpone GPU computation. The CubeCL trial passed
  correctness checks, but Rayon was simpler and faster at host-valued node
  boundaries. The trial does not rule out useful GPU acceleration later.
- **Requirement:** One manually selected preview detail applies to the whole
  graph; export uses full sensor detail. Navigation must not change computation.
  Preview latency below 300 ms is acceptable, not a reason to pursue every
  possible optimization.
- **Decided:** Evaluate all GUI nodes, including off-screen branches, so results
  and errors stay current without visibility-dependent scheduling. Unrelated
  branches remain usable when a node fails; export evaluates its dependencies.
- **Decided:** Keep CPU processing, texture preparation and large CPU image
  retirement off the UI thread. Show the previous completed view while working;
  current failures replace stale success. Only the latest requested state may
  become current, including across project replacement. Pending edits coalesce.
- **Decided:** Export captures the graph at the click and runs independently of
  later edits. Preview and export share decoded resources; invalidation or
  project replacement starts fresh without disrupting an existing export.
- **Requirement:** Cache decoded RAWs and derive only the requested mosaic.
  Retaining every normalized level costs too much memory for manual detail
  selection. Keep ordinary node-result reuse; release export intermediates when
  no longer needed instead of retaining a full-detail graph cache.
- **Decided:** Current resource costs are reasonable for prototype use. Memory
  budgets, streaming export and cancellation are deferred, as are failure-safe
  replacement, unsaved-change protection and waiting for exports on exit.

## Color and display

- **Requirement:** The prototype pipeline is RAW → white balance → 2×2 binning
  debayer → camera to linear Rec.2020 → sigmoid → TIFF. Bayer only for now.
  Full-detail export still has half the sensor dimensions because of binning.
- **Decided:** Preserve negative values after black subtraction to avoid biasing
  the noise floor. Normalize with a common denominator to avoid hidden white
  balance. Native black-level stages must not be mixed and subtracted twice.
- **Decided:** Follow dcraw's row-normalized camera matrix approach [1], targeting
  Rec.2020 directly. “No chromatic adaptation” means no separate CAT such as
  Bradford; neutral normalization still constitutes adaptation in camera space.
  Missing or invalid characterization is an input error.
- **Decided:** Keep the simple per-channel sigmoid with a fixed middle-grey point
  and soft shoulder. Highlight hue shifts and desaturation are accepted for the
  prototype; hue preservation and gamut mapping are future work.
- **Decided:** TIFF exports embed the selected RGB ICC profile, with common
  profiles built in. “Arbitrary ICC” means suitable RGB destinations, not every
  profile class. Float output does not guarantee unbounded transforms: profile
  curves/LUTs can clamp, and reader support varies.
- **Decided:** Let the compositor perform the display transform via scRGB.
  Owning the surface is why we use winit + wgpu + egui directly. Manually tagging
  a Rec.2020 surface relied on undocumented WSI behavior and saved no conversion.
  Extended values must survive the preview path; SDR white uses 203/80 scaling.
- **Decided:** Fall back to sRGB with a visible warning when scRGB is unavailable.
  egui's internal blending remains in gamma space. In-gamut screenshot checks
  passed; actual wide-gamut output still needs instrument verification.

## Editing and persistence

- **Decided:** The node canvas is the main workspace. Nodes own their views;
  parameters and views can open separately in OS windows. Let the window manager
  arrange them, without docking or saved pop-out state. Pop-outs should stay
  above the main window, rather than above every application.
- **Decided:** Use a neutral middle-grey surround and minimal decoration for
  judging images. Canvas text scales with the layer to avoid font-atlas churn;
  softness at large zoom is accepted. Idle windows should not redraw continually.
- **Decided:** The graph is the shared editing source of truth. Applying an edit
  also determines redraw and evaluation effects, so individual controls do not
  synchronize windows themselves. Presentation-only edits must not reprocess
  images. Worker snapshots are derived state, never a second editable project.
- **Decided:** Use human-readable JSON and keep frontend layout opaque to the
  library. Prototype files must match the current schema; compatibility,
  migration and silently filling missing parameters are deliberately excluded.
- **Decided:** Any node parameter may be a template input. Inputs belong to their
  individual nodes and use ordinary parameter storage; extracting a template
  resets those values to defaults. Shared binding/argument machinery was removed
  because it added rules without a current need.
- **Decided:** Save names ending in `.drip` unchanged; otherwise append `.drip`.
  Dialog filters do not enforce the suffix on every platform, and replacing an
  entered extension changes the requested name.
- **Open:** The repository uses AGPL-3.0-or-later; the original choice of AGPL
  left `-or-later` versus `-only` unconfirmed.

## Performance evidence

Historical measurements explain choices, not current performance guarantees.
On the Sony fixture, Ryzen 5600G (12 workers) and RTX 3080/Vulkan, the 2026-10-04
Rayon trial evaluated level 0 in about 162 ms versus 582 ms for CubeCL CPU and
623 ms for CubeCL GPU. All three used the same retained RAW pyramid.

After removing that pyramid, seven release level-change cycles gave medians of
244 ms at default 1/2 detail and 358 ms at Full. Decoded resource storage fell
from about 226 MB to 85 MB, excluding node results and temporary allocations.
These timings include evaluation and cache replacement, but exclude GUI
preparation/upload/drawing; filesystem caches were uncontrolled. The slower
Full transition was accepted with CPU work off the UI thread. Reproduce with
`examples/preview_latency.rs`; measure actual frames before tuning further.

## Reference

[1] D. Coffin, “dcraw.c,” `convert_to_rgb()` and `cam_xyz_coeff()`. [Online].
Available: https://www.dechifro.org/dcraw/
