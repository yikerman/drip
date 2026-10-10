# Design

Decisions not evident from code. Unfinished work is in [TODO](TODO.md), validation
in [PROGRESS](PROGRESS.md).

## Scope

- **Decided:** The next milestone is a packaged, user-testable proof of concept.
  Current image quality and editing workflows are accepted. Missing modules,
  broader reference-based validation and SpyderX measurements are not blockers.
  Prioritize installation, launch and a complete RAW-to-TIFF smoke test.
- **Requirement:** Correct display color on Wayland, macOS and Windows. Linux is
  the primary development platform; X11 is unsupported. A replacement GUI must
  cover all three color paths.
- **Decided:** Sub-300 ms latency is acceptable without further optimization.
  Evaluate the current CubeCL path with whole-node measurements, including
  transfers and allocations. Earlier Rayon comparisons are historical evidence,
  not the current backend policy.
- **Decided:** Prototype project formats have no migration guarantee.
- **Decided:** Inspector help treats Rustdoc source wraps as spaces and retains
  paragraph breaks, so prose wraps to the sidebar width. Node metadata retains
  the source line breaks; reflow belongs to presentation.

## RCD scratch

- **Decided:** Allocate scratch immediately before its first pass and release it
  after its final dispatch. CubeCL owns queued bindings and storage reuse; do
  not add host waits. This reduces Drip-owned scratch lifetimes without changing
  kernels. Runtime reservations and actual peak memory need separate measurement.

## Preview packing

- **Decided:** Pack preview pixels into disjoint RGBA f16 byte slices with Rayon.
  Pixel conversion is independent; preserve pixel order and native-endian bytes.
  Reuse the existing CPU pool and avoid splitting below 4096 pixels to amortize
  scheduling. Packing still completes before publishing the prepared view.

## Device scopes

- **Decided:** GUI observer ports request device RGB for histogram, waveform and
  vectorscope (including camera exposure scopes). Preparation uses the evaluator's
  client and reads back integer counts. Keep kernels in `drip-gui`; LittleCMS
  softproof/gamut check and preview packing keep their existing CPU path.
- **Decided:** Exposure scopes compare uploaded f32 thresholds to preserve exact
  bin edges. Vectorscope uses portable f32 arithmetic, normalizes RGB before XYZ
  to avoid HDR overflow and explicitly centers exact neutrals. Values very near
  bin boundaries can differ from the former f64 calculation; validate count
  conservation and bound drift against the reference. Scratch is image-size
  independent, with 32 chromaticity count shards (8 MiB).

## GUI actions

- **Decided:** Use sentence-case display labels independently of persisted keys.
  Registered IDs use `category.operation`, without migration aliases. Categories
  follow the agreed pipeline groups: Input, Sensor, Demosaic, Color, Tone,
  Geometry, View and Output. Names identify concrete operations and algorithms.
- **Decided:** Enum variants own their dependent parameter structs. Generate
  Serde adapters and recursive UI schemas together; use externally tagged JSON.
  Only the selected variant's fields are shown. Inactive variant edits may live
  in temporary UI state, but are not part of the saved parameter value.
- **Decided:** Projects use `.drip`; templates use `.drip-template`. Opening a
  template creates an untitled project. Exposed template parameters reset to
  their declared defaults when saving a template. No explanatory UI text is added.
- **Decided:** Keep declared payload names on ports and include endpoint names;
  mark optional canvas inputs with `?`. Elide long titles and expose full names
  on hover. Track unsaved persisted edits separately from evaluation requests.
- **Decided:** Scope overlays retain axes and compact color-space labels. Omit
  pixel/peak counts, dimensions and repeated detail/density settings; those
  settings already appear in the scope or global configuration.
- **Open:** Generate bound-source metadata and ports without losing immutable
  asset binding, shared output buffers or unavailable outputs. The current node
  macro assumes parameter-only state and allocated outputs; RAW is handwritten.
- **Decided:** Disable exports while asset bindings are pending, including cache
  invalidation reloads. Reject same-frame actions before snapshotting the graph;
  worker command ordering cannot refresh an immutable RAW already in a snapshot.
  The app commits binding notices before accepting further UI actions.

## Distribution

- **Decided:** Headless CI builds `drip/cpu` and runs tests with `DRIP_BACKEND=cpu`.
  GUI evaluation tests run normally; explicit GPU tests remain opt-in. Tests on
  every platform gate distribution. Distribution jobs only build and upload;
  they do not run tests or enable the CPU backend for testing.
  Clippy uses `--workspace --no-deps`; tests and doctests select only workspace
  packages. Dependencies still compile as required.
- **Requirement:** Minimize dependencies users must install. Native processing
  libraries should be static; the normal Linux C/C++ runtime may remain external.
- **Decided:** Vendor unmodified sources at release tags. Builds must not fetch
  or update sources; source pins and licenses belong in [THIRD_PARTY](../THIRD_PARTY.md).
- **Decided:** Use LibRaw's shared source manifest through `cc` on Windows too.
  Its MSVC makefile lists the same sources but hardcodes compiler/runtime and
  output choices that should remain Cargo's responsibility.
- **Decided:** Preserve user compiler flags and target selection. Defer CPU
  variants and OpenMP until measured gains justify their cross-platform build
  and runtime packaging costs.
- **Decided:** Build each distribution target natively to avoid cross-compiler,
  SDK and native-library sysroot setup. Use the newest standard GitHub runner
  images; this supersedes the older Ubuntu compatibility baseline.
- **Tentative:** Linux `.tar.xz`/Flatpak, unsigned macOS `.app` bundles in ZIPs,
  and unsigned Windows ZIPs. CI ZIPs are the initial delivery format.

## Future processing contracts

- **Requirement:** Sensor calibration needs matching sensor sites, CFA phase and
  sample units. Separate decoding from black subtraction/normalization/crop, or
  retain enough information to account for them. Spatial gains may require
  spatial saturation/noise metadata.
- **Requirement:** Chart calibration produces a reusable transform with a stated
  basis, normalization, reference illumination and fit residuals. Chart and
  subject can have different grids and exposures.
- **Requirement:** Masks, gain fields and scalar samples have distinct contracts
  despite shared storage. Range, broadcasting, division and geometry rules belong
  to consumers. Index-wise blending may pair unrelated scenes; sensor calibration
  requires sensor-site correspondence. Applying a mask after filtering differs
  from restricting the filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate.
  Processing must update or discard metadata whose assumptions it changes.
- **Decided:** Multiple-frame operations consume decoded-value collections.
  Alignment may be internal or reusable; no graph-level frame-stack abstraction
  is needed.
