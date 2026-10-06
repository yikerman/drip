# Design intent

Keep the intent and tradeoffs that code cannot explain. APIs, schemas, defaults
and algorithms belong in code and tests; usage belongs in README. Superseded
plans and detailed experiments remain in Git history. Unfinished work lives in
[TODO](../TODO.md), and verification limits in [PROGRESS](PROGRESS.md).

Statuses: **requirement** = user goal; **decided** = agreed direction;
**tentative** = proposal; **open** = unresolved. Decisions can be reconsidered
as the prototype develops; they are not permanent architectural constraints.

## Road to v0.1

**Decided (2026-10-05):** Work in this order:

1. Build a production-ready processing pipeline using proven algorithms from
   darktable, vkdt or Ansel as references. Adapt source where appropriate, retain
   attribution and license notices, and validate on real RAWs. The first ports are
   sigmoid, RCD and inpaint opposed; broader photographic validation remains.
2. Refine UI/UX around that pipeline until real editing flows well.
3. Verify color handling with the user's SpyderX colorimeter, alongside numerical
   processing and export checks. Instrument measurements validate the display
   path; they do not alone establish correctness of every processing algorithm.
4. Package and release v0.1. Development builds use `0.1.0-dev`.

This sequence takes priority over CLI expansion and speculative optimization.

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
- **Decided:** Use explicit semantic image contracts without adding plugin
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

## Node organization

- **Requirement (2026-10-05):** Node input/output tuples should be the single
  source of truth for edge compatibility and evaluation. Shared scopes should
  require image capabilities rather than borrow another node's accepted types.
- **Decided:** Use concrete semantic image types, traits for channel access
  and color interpretation, and typed node signatures. Derive runtime port
  descriptors and value adapters from those declarations; runtime editing still
  requires connection checks before values exist. Separate channel layout,
  linearity, color basis and scene/display meaning. Color characterization must
  stay attached to the data it describes.
- **Decided (2026-10-05):** Preserve existing dependency stamps and cache behavior
  through the type refactor, including preview-level invalidation. Dirty flags,
  revision keys and an execution-plan compiler are not prerequisites.
- **Decided:** Erased edge values do not require or expose payload equality.
  Cache reuse follows dependency stamps; concrete payloads may define comparisons
  for consumers that need them.
- **Decided:** Keep semantic laws beside the traits: channel storage, linearity,
  color interpretation and scene/display reference are distinct promises. Rust
  checks signatures; implementers remain responsible for the mathematical laws.
  Runtime capability registration exposes these promises without converting data.
  Preview presentation requires linear Rec.2020 because the shader assumes it.
  Associated input-borrow families take inspiration from
  [higher](https://github.com/bodil/higher); no dependency is needed.
- **Decided (2026-10-05):** Inputs may be declared optional in the kernel
  signature and arrive as `None` while unconnected. A connected optional source
  that fails still blocks the node, so a broken upstream is never silently ignored.

- **Decided:** Each backend node owns its schema, adapter and algorithm. Split
  substantial algorithms into local modules; keep small nodes in one file.
  Kernels take concrete data/settings and use Rust/Rayon, independent of the
  graph. Optional frontend node UIs reuse schema controls and shared editing;
  nodes without custom presentation require no frontend registration.
- **Decided:** Adapt darktable's sigmoid, RCD and inpaint-opposed algorithms in
  that order, with upstream attribution and independent/reference tests. Keep
  existing decoding, camera conversion and output paths; separate exposure from
  tone mapping. Scope initially remains Bayer.

- **Decided:** Normalize and reconstruct highlights at sensor resolution, then
  reduce the preview at demosaic input. This preserves clipping information and
  costs a full-sensor pass even for small previews. Demosaic kernels receive an
  ordinary mosaic; they do not know about the evaluator's preview setting.

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
- **Requirement:** Cache decoded RAWs without retaining a normalized pyramid.
  Sensor processing now requires a full-detail mosaic; demosaic adapters derive
  only the requested preview level and discard temporary levels. Keep ordinary node-result reuse; release export intermediates when
  no longer needed instead of retaining a full-detail graph cache.
- **Decided:** Current resource costs are reasonable for prototype use. Memory
  budgets, streaming export and cancellation are deferred, as are failure-safe
  replacement, unsaved-change protection and waiting for exports on exit.

- **Decided:** Carry conservative per-color saturation levels with each mosaic,
  scaling them with white balance. This keeps highlight detection in the same
  units as samples without inferring whether a WB node ran. Inpaint opposed
  uses as-shot-balanced data; no late chromatic adaptation is applied.

## Color and display

- **Requirement:** The prototype pipeline is RAW → white balance → highlight
  reconstruction → RCD → camera to linear Rec.2020 → exposure → sigmoid → TIFF.
  Bayer only for now; RCD exports preserve sensor dimensions.
- **Decided:** Preserve negative values after black subtraction to avoid biasing
  the noise floor. Normalize with a common denominator to avoid hidden white
  balance. Native black-level stages must not be mixed and subtracted twice.
- **Decided:** Follow dcraw's row-normalized camera matrix approach [1], targeting
  Rec.2020 directly. “No chromatic adaptation” means no separate CAT such as
  Bradford; neutral normalization still constitutes adaptation in camera space.
  Missing or invalid characterization is an input error.
- **Decided:** Use darktable-derived sigmoid color handling with fixed smooth
  Rec.2020 primaries, optional hue preservation, 0.18 middle grey and zero black.
  Keep exposure separate so it also applies to other display transforms.
- **Decided:** TIFF exports embed the selected RGB ICC profile, with common
  profiles built in. “Arbitrary ICC” means suitable RGB destinations, not every
  profile class. Float output does not guarantee unbounded transforms: profile
  curves/LUTs can clamp, and reader support varies.
- **Decided (2026-10-05):** TIFF export takes RAW metadata as an optional input
  and writes make/model and an Exif IFD with capture settings and time. Edges
  keep provenance explicit rather than hiding it in image types. Orientation is
  omitted because exported pixels are unrotated; LibRaw's normalized make is used.
- **Decided:** Composite egui and previews in an FP16 extended-sRGB canvas,
  retaining egui's encoded-space blending. Decode and limit the final composite
  to the SDR Rec.2020 volume, then output extended-linear BT.709 on all platforms.
  Negative/above-one BT.709 coordinates carry wide gamut. Preview also clips scene
  inputs in Rec.2020; processing and export are unchanged. HDR is deferred.
- **Decided:** Wayland uses Vulkan passthrough with a BT.709/extended-linear
  description, explicit Rec.2020 target primaries, and (0, 80, 80) cd/m² luminances
  [11, 12]. Require mastering-primaries and extended-target support, otherwise use
  sRGB. This keeps reference white independent of driver conventions. The pinned
  wgpu patch is documented in THIRD_PARTY.md [13].
- **Decided:** macOS uses wgpu's extended-linear sRGB layer tagging, which enables
  Metal EDR even for our SDR content. Windows uses the same shader and native
  scRGB as best effort, untested. HDR desktop SDR-white adjustment is not yet
  implemented. No additional wgpu patch is needed for the shared encoding.
- **Decided:** On Wayland request relative-colorimetric intent when advertised,
  otherwise perceptual, and log it. Other platforms use the system's mapping.
- **Requirement:** Fall back to bounded sRGB when the required output capabilities
  are absent. The matrix/shaper conversion is tested against LittleCMS relative
  output, including hardware-sRGB targets. Physical accuracy needs measurement.

- **Decided:** Diagnostic scopes use a black plotting area and light labels,
  distinct from the image preview’s middle-grey surround.
- **Decided:** Scopes inspect the connected node output. Waveforms retain image
  columns and measure RGB in EV, matching the histogram. Vectorscopes use
  exposure-independent CIE u′v′ centered on D65, with the input RGB space’s primary
  markers; omit black and clip negative channels only for this chromaticity visualization.
  Density colors are sRGB annotations of chromaticity, with brightness indicating
  count; out-of-gamut colors are clipped without moving their plotted positions.
  These conventions keep scene and display data interpretable without a video
  transfer function or an implicit display-profile conversion.

## Editing and persistence

- **Decided:** User-facing node help belongs below the type ID in the main
  inspector, outside parameter pop-outs. Keep descriptions technical and brief,
  with explicit assumptions. Port labels and help share frontend names for the
  kernel's contracts; the frontend does not redeclare connection compatibility.
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

With the new RCD/highlight pipeline, the same release harness and 12 workers
measured 1.20 s at 1/2 and 2.64 s at Full (seven level-change cycles,
2026-10-05). These rerun sensor processing and now produce four times as many RGB
pixels as the old binning pipeline. Exposure/sigmoid edits can reuse upstream
results; GUI preparation and drawing remain excluded. No optimization was added
solely to recover the old timings.

## Reference

[1] D. Coffin, “dcraw.c,” `convert_to_rgb()` and `cam_xyz_coeff()`. [Online].
Available: https://www.dechifro.org/dcraw/
