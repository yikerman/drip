# Design

Decisions and rationale. Development rules live in [AGENTS](../AGENTS.md),
contracts beside the code, validation in [PROGRESS](PROGRESS.md), unfinished work
in [TODO](../TODO.md). Status: requirement, decided, tentative, or open.

## Direction

- **Decided:** v0.1 priorities: production-pipeline validation, editing UI/UX,
  SpyderX verification, then packaging. CLI expansion and optimization follow.
- **Decided:** Frontend binaries use their package names: `drip-cli` and
  `drip-gui`. This keeps CLI Rustdoc output separate from the `drip` library.
- **Decided:** Keep `gpu-allocator` and the pinned `wgpu-hal` on the same
  `windows` crate version (currently 0.62.2); their DX12 interfaces cross crate
  boundaries. CI uses `--locked` to validate the committed dependency resolution.
- **Requirement:** Rust 1.95 is the minimum supported toolchain.
- **Requirement:** Correct display color on Wayland, macOS and Windows. Linux is
  the primary development platform; X11 is unsupported. A replacement GUI must
  cover all three color paths.
- **Requirement:** LibRaw handles decoding; LittleCMS handles ICC transforms.
- **Decided:** Use Rayon. The CubeCL trial lost on latency and development effort
  at host-valued node boundaries. Reconsider GPU execution with measured
  whole-node gains, including transfers and allocations. Sub-300 ms latency
  is acceptable without further optimization.

## DAG

- **Decided:** Concrete port types replace capability reflection and output
  inference. Working RGB uses linear Rec.2020/D65 throughout creative processing.
  Fixed output contracts remove downstream type propagation; local checks cover
  relationships between actual inputs.
- **Decided:** Export is an explicit action. Capture metadata arrives through a
  separate optional edge: provenance and pixel interpretation have different
  lifetimes.
- **Decided:** Keep strict project JSON and opaque frontend state; prototype
  formats have no migration guarantee. Template inputs are ordinary parameters.

### Future operations

These constraints guide future payloads; production implementations remain in TODO.

- **Requirement:** Sensor calibration needs matching sensor sites, CFA phase and
  sample units. Separate decoding from black subtraction/normalization/crop, or
  retain enough information to account for them. Spatial gains may require
  spatial saturation/noise metadata.
- **Requirement:** Chart calibration produces a reusable transform with a stated
  basis, normalization, reference illumination and fit residuals. Chart and
  subject can have different grids and exposures.
- **Requirement:** Masks, gain fields and scalar samples have distinct contracts
  despite shared storage. Range, broadcasting, division and geometry rules belong
  to the consuming operation. Index-wise blending may pair unrelated scenes;
  sensor calibration requires sensor-site correspondence. A mask applied after
  filtering differs from a mask restricting the filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate.
  Processing must update or discard metadata whose assumptions it changes.
- **Decided:** Multiple-frame operations consume decoded-value collections.
  Alignment may be internal or a reusable output; no graph-level frame-stack
  abstraction is needed.

## Processing and interaction

- **Decided:** Sensor processing precedes preview reduction to retain clipping
  information. Reduction belongs to demosaic integration. One manual detail level
  applies across the graph; navigation leaves it unchanged and export uses full
  detail.
- **Decided:** Evaluate every GUI consumer, including off-screen ones. Keep old
  views while work runs and replace them on current failure. The worker owns
  evaluation, preparation and large-image retirement to keep UI latency bounded.
- **Decided:** Exports retain the graph/resource snapshot captured at dispatch,
  including across project replacement. Preview/export share decoded resources;
  normalized pyramids and full-detail export graph caches are not retained.
- **Decided:** Canvas is the main workspace. Pop-outs are WM-arranged OS windows,
  with no docking or persisted pop-out layout. Canvas text scales with the layer,
  accepting softness at large zoom. Idle windows should rest.
- **Decided:** Cross-camera validation uses CC0 raw.pixls.us fixtures and the
  original Sony photograph. Git/LFS owns integrity; unusable metadata is tracked
  separately from unsupported decoders. Export parity alone cannot establish
  colorimetric accuracy.

## Logging

- **Decided:** Frontends configure stderr `log`, defaulting to
  `warn,drip=info,drip_gui=info`. Kernels return errors; the frontend owns reporting.

| Level | Events |
|-------|--------|
| Error | Failed explicit open/save/action, stopped worker, unusable display/window. |
| Warn | New live-preview failure, display fallback/recovery, unavailable parenting. |
| Info | Startup/version, GPU/output selection, project open/save, action start/completion. |
| Debug | Evaluation/timing, resources, invalidation, windows, incomplete configuration and refused edits. |
| Trace | Cache hits, discarded requests, frame timings and repaint scheduling. |

- **Decided:** Report each changed root preview failure once after accepting its
  generation; reset clears history. Action failures remain errors, including
  incomplete configuration. Retain dependency origin and relevant node, detail,
  generation, window or destination context. Display selection owns the fallback
  warning and its cause.
