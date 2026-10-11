# Design

Decisions not evident from code. Code principles are in `AGENTS.md`; node,
port and contract principles and their mathematics in
[TYPE_SYSTEM](TYPE_SYSTEM.md). Unfinished work is in [TODO](TODO.md),
validation in [PROGRESS](PROGRESS.md).

Status: **Requirement** is stated by the user; **Decided** is agreed;
**Tentative** and **Open** are unsettled and may be challenged.

## Scope

- **Decided:** The next milestone is a packaged, user-testable proof of concept.
  Scene-illuminant white balance and orientation are required additions. Other
  image-quality and editing limitations are accepted; broader reference
  validation and SpyderX measurements are not blockers. Verify installation,
  launch and a complete RAW-to-TIFF smoke test.
- **Decided:** “Scientific” for the prototype means sound, documented image
  processing, not calibrated radiometry or ACES conformance. Quantitative
  sensor/chart calibration is outside this milestone.
- **Requirement:** Correct display color on Wayland, macOS and Windows. Linux is
  the primary development platform; X11 is unsupported.
- **Decided:** Prototype project formats have no migration guarantee.

## Model gaps

Found in the 2026-10-10 review against [TYPE_SYSTEM](TYPE_SYSTEM.md) and
`AGENTS.md`. None is decided.

- **Open:** Interpretation is split across payload `TypeId` (Camera vs Color),
  string descriptor fields compared with `PartialEq`, and per-node contract
  code. Camera/Color reduction, histogram and waveform are duplicate nodes; the
  working-space check is repeated in five contracts and invisible to port
  labels; `Camera.coordinates` is a Debug-formatted matrix string.
- **Open:** Contracts receive `None` both for an absent optional input and for
  an upstream description not yet known. A `None` returned for a required output
  makes allocation fail generically, so the explanatory errors in Camera RGB
  conversion and Bayer site gains are unreachable.
- **Open:** `GuiNode` binds by string `ID`, restates `Parameters`, and
  re-extracts inputs by port name and type at runtime, duplicating the node
  declaration. File hints match string parameter paths. Candidate: bind by the
  generated node type and receive typed inputs derived from its signature.
- **Open:** TIFF export's node and action live in `drip-gui`, so a headless
  frontend cannot load or run it. The empty-node declaration is intended; where
  actions with headless use belong is not.
- **Open:** Parameter defaults are declared in `#[param]` and again in
  `impl Default`; node creation uses one, reset the other. Waveform copies
  `ExposureSettings` only to change its default scale.
- **Open:** `CaptureMetadata` carries the camera matrix and site gains, while
  independently usable values are meant to be separate ports. Its rkyv device
  codec has no device consumer; the CPU/GPU bijection principle and proportional
  abstraction conflict here.
- **Open:** Auto exposure and each export action create their own
  `RuntimeContext`, separate from the worker's evaluator. Nodes upload constants
  through the raw client, bypassing evaluator-owned transfer.
- **Open:** RAW is a handwritten `Node`; `loads_assets` and the failing default
  `parameters()` exist only for it. Generating bound-source declarations needs
  immutable asset binding, shared output buffers and unavailable outputs.

## Processing

- **Decided:** RAW exposes `mosaic: Bayer` and one owned `CaptureMetadata`:
  common capture/lens/GPS/sensor fields, native WB/color/black/white calibration,
  and optional derived site gains and camera matrix. Missing calibration does not
  hide descriptive metadata or mosaic data. Vendor maker notes are out of scope.
- **Decided:** Bayer site gains update samples and clipping levels in one node;
  both outputs declare the same target coordinates. Capture facts stay immutable
  and do not describe the geometry of a transformed image.
- **Decided:** Keep as-shot gains through highlights and demosaic, then fold
  reference/as-shot gain ratios into the camera matrix, following darktable's
  “as shot to reference” placement (THIRD_PARTY [22]). Derive the scene white
  from the mapped camera neutral and apply CAT16 in linear Rec.2020 before
  rendering. The node accepts metadata or custom source xy; target white is D65.
- **Decided:** Start with full CAT16 adaptation, composed into one working-RGB
  matrix. Full adaptation needs no scene luminance or surround estimate. Partial
  adaptation needs a policy: generalized/two-step von Kries keeps inverse and
  composition consistency [7]; WGM models incomplete adaptation differently [8];
  high-chroma experiments expose limits in all six tested CATs [9]. Keep model
  choice separate from illuminant estimation; temperature/tint is a white-point
  UI, not a CAT. Full CAM16/Hellwig modeling and mixed illuminants are separate [4], [5].
- **Decided:** EXIF orientation travels in capture metadata so contracts know
  the oriented extent before allocation. Decode LibRaw's convention at the RAW
  boundary; unknown means identity. Orient once after camera conversion; Bayer
  processing keeps sensor coordinates. TIFF omits the original orientation tag.
- **Decided:** Exposure scaling is distinct from sensor-white normalization; the
  RAW denominator and sigmoid's 0.18 pivot do not establish ACES's 18% reflector
  scale [6]. Auto exposure is a one-shot GUI action that meters linear Rec.2020
  mean luminance on the device and adds `log2(0.18 / mean)` to EV through the
  ordinary parameter edit, synchronously. Mean-to-18% is a starting point, not
  calibration.
- **Decided:** Signed/HDR coordinates are valid before rendering [3]. Sigmoid is
  a rendering operation although its output stays identity-encoded; LittleCMS
  with the selected intent handles output conversion. ACES target-aware
  rendering and source gamut compression are references, not prototype
  requirements [1], [2]. The Rec.2020/D65 working space stays for the prototype.
- **Decided:** Missing camera calibration is a supported-input limitation, not a
  reason to invent a matrix. Keep absent calibration distinct from extraction
  failures (Sigma DNGs have ColorMatrix tags but zero LibRaw `cam_xyz`).
- **Decided:** TIFF depth is u8, u16 (default) or f32. LittleCMS converts into
  the output encoding before quantization; depth selects no transfer function.
- **Decided:** rkyv 0.8 for the metadata byte transport, with validation and
  unaligned archives. Bincode was rejected as unmaintained.
- **Tentative:** Explain the scene-adjustment → rendering → encoding order in
  help and templates without an ordering graph check.
- **Requirement:** Sensor calibration needs matching sites, CFA phase and sample
  units; separate decoding from black subtraction/normalization/crop or retain
  enough to account for them. Spatial gains may need spatial saturation/noise data.
- **Requirement:** Chart calibration produces a reusable transform with stated
  basis, normalization, reference illumination and fit residuals. Chart and
  subject may differ in grid and exposure.
- **Requirement:** Masks, gain fields and scalar samples have distinct contracts
  despite shared storage. Range, broadcasting, division and geometry rules belong
  to consumers. Applying a mask after filtering differs from restricting the
  filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate;
  processing updates or discards metadata whose assumptions it changes.
- **Tentative:** Lens correction needs lens identification, crop factor and
  correction profiles; vignetting may use focus distance, not minimum focus
  distance. Place correction before orientation or account for transformed
  coordinates.
- **Decided:** Multiple-frame operations consume decoded-value collections;
  no graph-level frame-stack abstraction.

## Compute

- **Decided:** CubeCL computation with one kernel source per algorithm across
  runtimes; wgpu presentation and color management stay frontend-owned.
  `DRIP_BACKEND` selects explicitly and never falls back (crate README).
- **Requirement:** Do not distribute the CPU runtime; its LLVM JIT is too large.
  Windows uses D3D12. Distribution builds use the native candidates: Vulkan
  SPIR-V on Linux, native Metal on macOS, WGSL/D3D12 on Windows; xtask checks
  the target platform. These are performance candidates, not measured winners.
- **Decided:** RCD scratch is allocated before its first pass and released after
  its last dispatch; CubeCL owns queued bindings. No host waits.
- **Decided:** Reductions use fixed-order partial sums without float atomics.
  Exposure reductions cap lanes at the runtime limit; CPU barrier kernels need a
  host worker per lane. Scopes compare uploaded f32 thresholds to keep exact bin
  edges; vectorscope normalizes RGB before XYZ for HDR headroom.
- **Decided:** GUI scopes and auto exposure consume device RGB with GUI-owned
  kernels and read back counts or one f32. Softproof, gamut check and preview
  packing stay on the CPU; packing uses Rayon over disjoint RGBA f16 slices.

## Performance

- **Requirement:** No persistent intermediate cache; do not optimize prematurely.
  Tracing adds no synchronization. Benchmark artifacts stay outside Git.
- **Decided:** Sub-300 ms latency is acceptable without further optimization.
- **Decided:** Benchmark protocol: Sony fixture (7968×5320), release builds,
  WGPU on the RTX 3080, `RAYON_NUM_THREADS=12`, serial runs. Drive the real GUI
  worker with Preview, Histogram, Waveform, Vectorscope and Export targets.
  Warm up levels 2, 1, 0; repeat them five times; alternate ±0.1 EV five times
  at level 1; issue five identical level-1 requests. Report medians and ranges;
  exclude drawing, texture upload and file export. Harness and results live in
  `../drip-benchmark-results/`.
- **Open:** Bounded reuse for exposure/sigmoid edits conflicts with the no-cache
  requirement; it would need a byte budget and context/resource invalidation.

## GUI

- **Requirement:** Preserve universal node controls/views, per-node pop-out
  windows and interaction logic; internal APIs may change.
- **Decided:** Sentence-case display labels independent of persisted keys. IDs
  use `category.operation` without migration aliases. Categories: Input, Sensor,
  Demosaic, Color, Tone, Geometry, View, Output.
- **Decided:** Enum variants own their dependent parameter structs; Serde
  adapters and UI schemas are generated together, externally tagged. Only the
  selected variant's fields are shown and saved.
- **Decided:** Projects use `.drip`, templates `.drip-template`. Opening a
  template creates an untitled project; exposed template parameters reset to
  their defaults when saving a template.
- **Decided:** Ports show declared payload names and endpoint names; optional
  inputs are marked `?`. Long titles are elided with full names on hover.
  Unsaved edits are tracked separately from evaluation requests.
- **Decided:** Scope overlays keep axes and compact color-space labels; no
  counts, dimensions or repeated settings. Waveforms default to logarithmic
  density, histograms to linear.
- **Decided:** Inspector help joins Rustdoc source wraps and keeps paragraphs;
  reflow belongs to presentation.
- **Decided:** Exports are disabled while asset bindings are pending. Same-frame
  actions are rejected before snapshotting the graph.
- **Decided:** The default RAW template uses a landscape layout: views share a
  top edge, processing staggers down through Orientation, RAW and TIFF sit at
  the bottom corners.
- **Tentative:** Rounded orthogonal wires could help backward links; routing
  around nodes needs obstacle-aware paths. No router chosen.

## Distribution

- **Decided:** Headless CI builds `drip/cpu` and tests with `DRIP_BACKEND=cpu`;
  GPU tests are opt-in. Tests on every platform gate distribution; distribution
  jobs only build and upload. Clippy uses `--workspace --no-deps`.
- **Requirement:** Minimize user-installed dependencies; native processing
  libraries are static, the normal Linux C/C++ runtime may stay external.
- **Decided:** Vendor unmodified sources at release tags; builds never fetch.
  Pins and licenses are in [THIRD_PARTY](../THIRD_PARTY.md). LibRaw uses its
  shared source manifest through `cc` on Windows too.
- **Decided:** Preserve user compiler flags and targets. Defer CPU variants and
  OpenMP until measured gains justify their packaging cost.
- **Decided:** Build each target natively on the newest standard GitHub runners.
- **Tentative:** Linux `.tar.xz`/Flatpak, unsigned macOS `.app` ZIPs, Windows ZIPs.

## References

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

[4] C. Li et al., “Comprehensive color solutions: CAM16, CAT16, and CAM16-UCS,”
Color Research & Application, vol. 42, no. 6, pp. 703–718, 2017,
doi: <https://doi.org/10.1002/col.22131>.

[5] L. Hellwig and M. D. Fairchild, “Brightness, lightness, colorfulness, and
chroma in CIECAM02 and CAM16,” Color Research & Application, vol. 47, no. 5,
pp. 1083–1095, 2022, doi: <https://doi.org/10.1002/col.22792>.

[6] S. Tobenkin, “IDTs: How Do They Work,” ACESCentral, Apr. 15, 2020.
[Online]. Available:
<https://acescentral.com/knowledge-base-2/idts-how-do-they-work/>.
Accessed: Oct. 10, 2026.

[7] C. Gao et al., “The development of the CIECAM16 and visualization of its
domain and range,” Color Research & Application, 2025,
doi: <https://doi.org/10.1002/col.22959>.

[8] C. Shen and M. D. Fairchild, “Weighted Geometric Mean (WGM) method: A new
chromatic adaptation model,” PLOS ONE, vol. 18, no. 8, e0290017, 2023,
doi: <https://doi.org/10.1371/journal.pone.0290017>.

[9] Y. Shen et al., “Performance of Chromatic Adaptation Transforms for Stimuli
at Different Chroma Levels,” Color Research & Application, vol. 51, no. 4,
e70108, 2026, doi: <https://doi.org/10.1002/col.70108>.
