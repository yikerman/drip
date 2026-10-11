# Design

Decisions not evident from code. Unfinished work is in [TODO](TODO.md), validation
in [PROGRESS](PROGRESS.md).

## Scope

- **Decided:** The next milestone is a packaged, user-testable proof of concept.
  Scene-illuminant white balance and orientation are required additions. Other
  current image-quality and editing limitations are accepted; broader reference
  validation and SpyderX measurements are not blockers. Verify installation,
  launch and a complete RAW-to-TIFF smoke test.
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

- **Decided:** Exposure reductions cap workgroup lanes at the runtime's reported
  limit and distribute all bins across those lanes. CPU barrier kernels require
  a host worker per lane; a fixed GPU-sized group oversubscribes CI machines.
  Keep the same reduction implementation and 256-lane GPU launches.
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

- **Decided:** Exposure's custom GUI adds a one-shot Auto exposure button. It
  evaluates through that node at full detail, averages linear Rec.2020 luminance
  and adds `log2(0.18 / mean)` to the current EV through the ordinary parameter
  edit path. This reaches the same fixed point as metering at zero EV, within
  floating-point rounding. The control evaluates synchronously; it introduces
  no worker action or automatic processing mode. The average-grey assumption is
  a content-dependent starting point, not calibrated scene reflectance.
- **Decided:** Auto exposure requests `Device<ColorRgb>` and reduces mean
  luminance with a GUI-owned CubeCL kernel, shared across compute backends.
  Normalize sample contributions before bounded reductions for HDR headroom;
  read back one f32 rather than the full image. The EV calculation remains on
  the host, and the button retains its synchronous behavior.
- **Tentative:** The refreshed ACES comparison identified editable scene-illuminant
  adaptation as the remaining required processing feature; CAT16 now implements it.
  Normalization, coupled sample/clipping gains, highlight reconstruction,
  demosaic, camera conversion, orientation, exposure, sigmoid and ICC TIFF output
  are implemented, including device mean-to-18% auto exposure. Orientation and
  auto exposure and chromatic adaptation still need the user's interactive acceptance;
  distribution still needs clean-desktop checks for each advertised platform.
  This assessment uses the agreed supported-Bayer/SDR scope, not ACES conformance.
- **Decided:** Initial WB offers an as-shot starting point and manual source xy
  before sigmoid. Temperature/tint and neutral sampling remain follow-ups. Check consistent sample
  and clipping coordinates, persisted controls and matching preview/export.
  CAT16 is the chosen implementation, not an ACES requirement. ACES 2's
  simplified Hellwig model belongs to its output rendering [1].
- **Decided:** Keep exposure scaling distinct from sensor-white normalization.
  The current RAW denominator and zero-EV default do not establish ACES's
  calibrated 18% scene-reflector scale [7]; sigmoid's 0.18 pivot does not supply
  that calibration. Manual exposure and one-shot mean metering suffice for the prototype. Automatic
  baseline/exposure normalization and cross-camera matching remain follow-ups.
- **Tentative:** The ACES comparison supports keeping the current Rec.2020/D65
  pipeline for the packaged proof of concept. ACES adoption is a separate scope:
  changing primaries alone does not supply a calibrated input transform or the
  ACES rendering transform. Existing image-quality acceptance still applies.
- **Requirement:** Add scene-illuminant chromatic adaptation and photo orientation
  for the first usable prototype. Both are implemented, pending interactive checks.
- **Decided:** Keep as-shot gains through highlights and demosaic, then fold
  reference/as-shot gain ratios into the prepared camera matrix. This follows
  darktable's current "as shot to reference" placement (THIRD_PARTY [22]); it
  supersedes the earlier proposal to change upstream conditioning. Bayer gains,
  clipping levels and reconstruction keep their existing convention. Derive
  the scene white from that matrix's mapped camera neutral, then apply CAT16
  in linear Rec.2020 before rendering. The separate node accepts metadata or
  custom source xy; target white is D65. No evaluator or metadata layout changes.
  Existing graphs need this node after camera conversion; no automatic migration.
  Full CAM16/Hellwig appearance
  modeling, mixed-illuminant masks and chart fitting remain separate work [5], [6].
- **Decided:** Start with full adaptation using the CAT16 response matrix;
  compose the white-point transform into one working-RGB matrix and reuse the
  existing CubeCL matrix kernel. Full adaptation neutralizes the chosen scene
  white without requiring scene luminance or observer-surround estimates.
  Partial adaptation needs a separate policy: generalized von Kries/two-step
  formulations preserve inverse/composition consistency [8], while WGM offers
  a different model of incomplete adaptation [9]. Neither establishes a
  universally best photographic result. Recent high-chroma experiments expose
  limitations in all six tested standard CATs [10]. Keep model selection and
  illuminant estimation separate; temperature/tint is a white-point UI, not a CAT.
- **Decided:** Carry EXIF orientation in capture metadata and its description so
  contracts can determine the oriented extent before allocation. Decode LibRaw's
  transpose/flip convention at the RAW boundary; unknown orientation means
  identity. Apply rotation/reflection once after camera-to-color conversion in
  the starting template, preserving sensor coordinates through Bayer processing.
  The Orientation parameter defaults to metadata and may replace it with any of
  the eight EXIF transforms. Missing metadata means identity. Shared schema
  controls provide the GUI override without a duplicate frontend declaration.
  Preview and export consume the same oriented image; TIFF omits the original
  orientation tag to prevent a second transform. Graphs are not upgraded automatically.
- **Decided:** LittleCMS with the selected rendering intent is sufficient for
  prototype output. Additional target-gamut rendering is deferred. ACES 2
  integrates target-aware tone/chroma/gamut mapping into rendering [1], while its
  reference gamut compression addresses source colors [2]; neither is required
  for this milestone.
- **Decided:** Missing camera calibration is a supported-input limitation, not a
  requirement to invent a matrix or build a camera-profile database. Preserve
  the distinction between absent calibration and extraction failures: the known
  Sigma DNGs contain ColorMatrix tags despite zero LibRaw `cam_xyz` output.
- **Tentative:** Explain the recommended scene-adjustment → rendering → encoding
  order in node help/templates without imposing an artistic-order graph check.
  Identity encoding alone does not imply scene-referred values after sigmoid.
  Negative and above-one color coordinates are not inherently invalid; ACEScg
  explicitly permits them [3]. Existing consumer contracts still apply.

- **Decided:** RAW exposes Bayer and one owned CaptureMetadata value. It contains
  common capture/lens/GPS/sensor fields, native WB/color/black/white calibration,
  and optional derived site gains and camera matrix with explicit interpretations.
  Missing usable calibration does not hide descriptive metadata or mosaic data.
  Private vendor maker-note structures are outside the requested scope.
- **Decided:** Bayer site gains update samples and clipping levels in one node;
  both outputs declare the same target coordinates. Camera conversion reads its
  matrix from metadata. Capture facts stay immutable and are not interpreted as
  the geometry of a transformed image. Existing prototype graphs using the old
  calibration ports require recreation; no automatic migration is provided.
- **Decided:** Use rkyv 0.8 for the metadata payload's required byte transport,
  with byte validation and unaligned archives. CPU consumers share owned values;
  kernels receive only their small coefficient buffers. This is not persistence
  or a GPU shader struct. Bincode was rejected because it is unmaintained.

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
- **Tentative:** Lens correction needs lens identification, camera crop factor
  and correction profiles beyond capture metadata; vignetting may also use
  exposure focus distance, which is not the lens minimum focus distance. Common
  LibRaw lens fields are retained, preserving unknown values. Keep image-to-capture geometry separate from capture
  facts; place correction before Orientation in the recommended template, or
  account explicitly for transformed coordinates. Metadata export alone cannot
  supply a distortion/TCA/vignetting model.
- **Decided:** Multiple-frame operations consume decoded-value collections.
  Alignment may be internal or reusable; no graph-level frame-stack abstraction
  is needed.

### ACES review references

[1] Academy of Motion Picture Arts and Sciences, “Overview of Output Transform
Structure,” ACES Documentation, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/system-components/output-transforms/technical-details/rendering-overview/>.
Accessed: Oct. 10, 2026.

[2] Academy of Motion Picture Arts and Sciences, “Reference Gamut Compression,”
ACES Documentation, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/rgc/overview/>. Accessed: Oct. 10, 2026.

[3] Academy of Motion Picture Arts and Sciences, “ACEScg — A Working Space for CGI
Render and Compositing,” ACES Documentation, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/encodings/acescg/>. Accessed: Oct. 10, 2026.

[4] darktable contributors, “Color calibration,” darktable development user
manual. [Online]. Available:
<https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/color-calibration/>.
Accessed: Oct. 10, 2026.

[5] C. Li et al., “Comprehensive color solutions: CAM16, CAT16, and CAM16-UCS,”
Color Research & Application, vol. 42, no. 6, pp. 703–718, 2017,
doi: <https://doi.org/10.1002/col.22131>.

[6] L. Hellwig and M. D. Fairchild, “Brightness, lightness, colorfulness, and
chroma in CIECAM02 and CAM16,” Color Research & Application, vol. 47, no. 5,
pp. 1083–1095, 2022, doi: <https://doi.org/10.1002/col.22792>.

[7] S. Tobenkin, “IDTs: How Do They Work,” ACESCentral, Apr. 15, 2020.
[Online]. Available:
<https://acescentral.com/knowledge-base-2/idts-how-do-they-work/>.
Accessed: Oct. 10, 2026.

[8] C. Gao et al., “The development of the CIECAM16 and visualization of its
domain and range,” Color Research & Application, 2025,
doi: <https://doi.org/10.1002/col.22959>.

[9] C. Shen and M. D. Fairchild, “Weighted Geometric Mean (WGM) method: A new
chromatic adaptation model,” PLOS ONE, vol. 18, no. 8, e0290017, 2023,
doi: <https://doi.org/10.1371/journal.pone.0290017>.

[10] Y. Shen et al., “Performance of Chromatic Adaptation Transforms for Stimuli
at Different Chroma Levels,” Color Research & Application, vol. 51, no. 4,
e70108, 2026, doi: <https://doi.org/10.1002/col.70108>.
