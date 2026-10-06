# Design intent

Decisions and rationale; detailed contracts live beside the code. Unfinished work
is in [TODO](../TODO.md), validation evidence in [PROGRESS](PROGRESS.md), and
algorithm credits in [THIRD_PARTY](../THIRD_PARTY.md).

Statuses: **requirement** = user goal; **decided** = agreed direction;
**tentative** = proposal; **open** = unresolved.

## Direction and boundaries

- **Decided:** v0.1 work proceeds through production-pipeline validation, editing
  UI/UX, SpyderX color verification, then packaging. CLI expansion and speculative
  optimization follow those priorities.
- **Requirement:** Processing, graph validation, color management and persistence
  belong in the GUI-independent library. Linux/Wayland is primary; macOS and
  Windows should build in CI. X11 is unsupported.
- **Requirement:** Use LibRaw for decoding and LittleCMS for ICC transforms.
  The minimal owned-data [LibRaw binding](../crates/drip-libraw/src/lib.rs) keeps
  version-dependent native layouts behind a C shim.
- **Requirement:** Group source by semantic responsibility, not file size.
  Keep one computational kernel source across backends; portability and ease of
  development take priority over peak throughput.
- **Decided:** Nodes own their schemas, adapters and kernels. Demosaicing and
  scopes form local families; GUI rendering and shared node controls have their
  own homes. Keep cohesive implementations together.

## Processing and graph contracts

- **Requirement:** Typed input/output tuples are the source of connection and
  evaluation contracts. Consumers request the weakest meaningful capability;
  see [image laws](../crates/drip/src/image.rs),
  [port adapters](../crates/drip/src/ports.rs) and
  [runtime registration](../crates/drip/src/value.rs).
- **Decided:** Separate scene/display meaning from color basis, and retain camera
  characterization with pixels. Do not track WB history in types: graph
  flexibility is preferred over preventing double WB.
- **Decided:** Evaluation has no output side effects; file writes are explicit
  actions. Preserve dependency-stamp caching, including preview-level
  invalidation, without requiring payload equality. Cache lifetimes and optional
  input failure semantics are documented in [eval](../crates/drip/src/eval.rs).
- **Requirement:** The initial Bayer pipeline is RAW → WB → highlights → RCD →
  camera to Rec.2020 → exposure → sigmoid → TIFF. Exposure remains separate from
  tone mapping. Adapted algorithms retain local notices, deviations and tests.
- **Decided:** Preserve negative sensor samples and avoid implicit WB during
  [normalization](../crates/drip/src/nodes/raw.rs). Use dcraw-style camera matrix
  normalization without a separate CAT; see [color](../crates/drip/src/color.rs).
- **Decided:** Sensor processing precedes preview reduction, preserving clipping
  information at the cost of full-sensor passes for small previews. Reduction
  belongs to [demosaic integration](../crates/drip/src/nodes/demosaic/mod.rs),
  keeping kernels independent of evaluation detail.
- **Decided:** Export metadata travels through an explicit optional edge, keeping
  provenance separate from image semantics. TIFF/profile behavior is defined in
  [export](../crates/drip/src/nodes/export.rs).

## Responsiveness

- **Decided:** Use Rayon. The CubeCL trial passed correctness checks but was
  slower and harder to develop at host-valued node boundaries. GPU work needs
  measured whole-node benefit, including transfers and allocation.
- **Requirement:** One manual preview detail applies across the graph; export
  uses full detail. Navigation must not change processing. Latency below 300 ms
  is acceptable without further optimization.
- **Decided:** Evaluate all GUI nodes so off-screen results and errors stay
  current. Keep CPU evaluation, preparation and large-image retirement off the
  UI thread. Coalesce edits, publish only the latest requested preview, retain
  old views while working, and replace them on current failure.
- **Decided:** Export captures the graph at the click and survives later edits
  or project replacement. Preview/export share decoded files, without retaining
  a normalized pyramid or a full-detail export graph cache. Scheduling lives in
  [worker](../crates/drip-gui/src/worker.rs).
- **Decided:** Current prototype resource costs are acceptable. Robust saves,
  close protection, cancellation and memory budgets remain deferred in TODO.

## Presentation and editing

- **Decided:** Canvas and node gestures separate navigation from modification.
  Left-drag pans the whole canvas even when started on a node. This decision
  covers only the canvas background and node body.

  | Context | Left click | Left drag | Right click | Right drag |
  |---------|------------|-----------|-------------|------------|
  | Empty canvas | Clear selection | Pan canvas | Open Add node menu | No action |
  | Node | Select and show inspector | Pan canvas | Open Rename / Delete menu | Move node |

  The mouse wheel zooms around the pointer in both contexts. Rename selects
  the node and focuses its existing label field in the inspector.

- **Decided:** Right-click menus use a lighter neutral fill and a dark border
  to separate them from the middle-grey workspace; styling is shared in
  [theme](../crates/drip-gui/src/theme.rs).
- **Decided:** Use one extended-linear BT.709 output pipeline with an SDR Rec.2020
  bound and bounded-sRGB fallback. Explicit Wayland tagging avoids driver-defined
  reference white; macOS/Windows use native scRGB. Encoding, blending and platform
  contracts are in [display](../crates/drip-gui/src/render/display.rs); the wgpu
  patch and removal condition are in THIRD_PARTY. HDR remains deferred.
- **Decided:** Scopes inspect connected data without a display-profile transform:
  EV for channel exposure, exposure-independent u′v′ for chromaticity. Their
  [kernels](../crates/drip/src/nodes/scopes/density.rs) and
  [drawing](../crates/drip-gui/src/render/node_views.rs) define the coordinates
  and annotation colors. Scopes use black; image previews use middle grey.
- **Decided:** The canvas is the main workspace. Parameters and views can pop
  out into OS windows above the main window; the WM arranges them. No docking or
  persisted pop-out state. Scale canvas text with its layer to avoid atlas churn,
  accepting softness at large zoom; idle windows should not redraw continually.
- **Requirement:** Technical node help appears below the type ID in the main
  inspector, outside pop-outs. Frontend labels describe backend contracts rather
  than redefining compatibility; see [node UI](../crates/drip-gui/src/node_ui/).
- **Decided:** The graph is the sole editable state. Shared
  [editing](../crates/drip-gui/src/editing.rs) determines redraw/evaluation effects;
  presentation edits do not reprocess images.
- **Decided:** Use strict, human-readable JSON with opaque frontend layout and
  no prototype migration. Template inputs use ordinary per-node parameters,
  avoiding separate binding machinery; see [project](../crates/drip/src/project.rs).
- **Open:** AGPL-3.0-or-later is recorded, but the original `-only` versus
  `-or-later` choice remains unconfirmed.
