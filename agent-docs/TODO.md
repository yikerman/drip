# TODO

Unfinished work. Deferred ideas need a concrete use or measurement.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Compute model coverage | Follow-up | Deferred shapes read only `Rgb`, and warps are projective. Other payloads, lens distortion and many-frame foreign inputs wait for a node that needs them. |
| Waveform pass | Follow-up | The generic bins pass counts a level-1 waveform in 2.3 ms against 1.6 ms for the per-column kernel it replaced. A bins body that confines counts to a column could accumulate per workgroup in shared memory. |
| Scope output ports | Open | The scopes' `Counts` output shows as a port no node accepts. Decide whether the canvas hides outputs nothing consumes. |
| Reduction readback | Follow-up | Each reduction reads its result back before the next node runs, so three scopes wait three times. Keep results on the device until read, and measure. |
| Kernel halos | Open | Opaque kernels run whole, so a 100 % crop computes the whole RCD once per graph. A `halo` declaration needs lazily run kernels or a describing pass, and CFA-aligned Bayer windows ([DESIGN](DESIGN.md#compute)). |
| Stencil load fusion | Idea | Maps before a stencil run in a launch of their own. Fusing them into the taps recomputes them `(2R + 1)²` times, so measure first. |
| Separable stencils | Idea | Sharpen reads 121 taps per sample at its default reach, 14 ms over the tail at full detail. A row and a column pass would read `2(2R + 1)`, as a stencil shape or a meta node of two stencils. |
| Two-image maps | Open | A map reads one image, so Pyramid blur cannot blend its blur with its input, which local contrast and masked edits need. A body over a pixel of several images on one lattice would fuse such a blend. |
| Meta expansion | Follow-up | Sub-graph nodes are absent from `Dag::order`, so a frontend cannot target them. Revisit when a meta node needs foreign or meta nodes inside, or when contracts run before evaluation. |
| Bayer stencils | Open | Hot pixels and demosaic passes as stencils need windows aligned to CFA cells and a lattice per phase. |
| Tiled export | Follow-up | Whole kernels upstream still allocate the whole image on the device, and the TIFF encoder takes one host buffer because tiff 0.11.3's `write_strip` ignores compression. Stream strips when the encoder allows it, and band JPEG export too. |
| Foreign job cancellation | Open | A job no demand needs runs to its end. Add a cooperative check when a foreign node can stop early, such as a denoiser. |
| Disk tier reads | Follow-up | A disk hit reads on the evaluating thread, 89 ms for the Sony mosaic from the page cache. Measure a cold read before moving reads to the job thread. |
| Store accounting | Follow-up | Entries sharing storage are charged twice, and budgets bound neither a running evaluation's values nor untaken job outputs. Measure device memory over a long session first. |
| Benchmark protocol | Open | Confirm with the user whether the protocol adopts the five new-EV requests the store measurements added. |
| Step 3 measurement | Follow-up | Time the template tail with Orientation, and without geometry, against 17142d6 in `../drip-benchmark-results/2026-10-13-fusion/`. |
| Raster runtime identity | Follow-up | `Raster::device` does not check that a later caller uses the runtime that made the buffer, and `Statistics` mixes concurrent evaluations on one runtime. Check or key it if a store ever serves several runtimes. |
| Remaining simplifications | Follow-up | Revisit preparation and kernel contexts, colorimetry helpers, the evaluation loop and preview-context ownership. A scope's density scale is a parameter, so changing it counts again. |
| Packaged proof of concept | Next | Verify saved edits, full-detail RAW-to-TIFF use, and installation and launch on clean desktops. Record supported environments and known limitations. |
| Portable artifacts | Next | Add Linux `.tar.xz`/Flatpak and macOS `.app` bundles with their minimum systems and runtime packaging. Audit runtime-loaded dependencies, permissions, submodule sources and notices. |
| Build portability | Open | Verify Rust 1.95 and full native Windows and macOS builds. |
| Display portability | Open | Verify Windows and macOS tagging, reference white, monitor changes and Advanced Color, plus other GPUs, compositors and HDR white. Legacy Windows needs ICC output. |
| Image/color validation | Follow-up | Review detail, highlights and colour against measured targets, and the display path with SpyderX. Not a proof-of-concept gate. |
| Scene-illuminant white balance | Integration test pending | A user check verifies that saved overrides reopen unchanged and full-detail preview and export agree. |
| Photo orientation | Integration test pending | A user check verifies preview, override, reopening and TIFF orientation. |
| Auto exposure | Integration test pending | A user check verifies real-photo response, repeated clicks, saved EV and preview/export agreement. |
| Perspective Transform view | Integration test pending | A desktop session verifies the handles, Keep inside and Fit. |
| White-balance controls | Follow-up | Add temperature/tint and neutral sampling to the CAT16 controls, which accept source xy today. |
| Partial chromatic adaptation | Deferred | Decide between mood preservation and appearance prediction before adding a degree control, and compare generalized CAT16 with WGM on photographs. |
| Exposure normalization | Deferred | Neither manual exposure nor metering calibrates an 18 % reflector. Cross-camera matching needs a defined convention and validation. |
| Output-gamut rendering | Deferred | Revisit target-gamut rendering when images warrant it, separating source compression from output mapping. |
| CJK text | Deferred | Embedded fonts cover Latin, Greek and Cyrillic. Prefer a system-font fallback over embedding Source Han Sans. |
| Editing UI/UX | Follow-up | Refine from tester feedback. |
| Remaining frontend presentation | Open | Export depth and intent terminology, EV reference annotations, proof-mode badges, error elision and blank or stale pop-out views. |
| RAW metadata extraction | Open | Canon EOS D30 (#1307) and Samsung GX-1L (#8115) have unusable white balance, the latter a zero camera matrix. Both are excluded from the corpus, and IDs refer to raw.pixls.us. |
| DNG matrix extraction | Open | Sigma fp DNGs (#7280, #7273) have ColorMatrix tags, but LibRaw returns a zero `cam_xyz`. Excluded from the corpus. |
| Panic-time native teardown | Open | Investigate the NVIDIA/Wayland segmentation fault during unwind after the image-view panic. |
| Worker startup failures | Open | Report thread-creation failures in `Worker::new` without losing the worker. |
| Sensor calibration | Open | Expose sample conventions and sensor coordinates before dark and flat correction ([DESIGN](DESIGN.md#processing)). |
| Everyday image nodes | Proposed | Implement in the order under [Next image nodes](#next-image-nodes), outside the proof-of-concept gate. |
| Geometry template | Follow-up | Decide whether to list `templates/straighten-and-resize.drip-template` among the built-in templates. |
| Geometry extensions | Open | Single-channel geometry, maps shared by an image and its mask, registration onto another grid, EWA or a prefiltered pyramid beyond 16× reduction, and a derivation of repeated-resampling loss ([FIELD](FIELD.md#open-questions)). |
| Later processing extensions | Deferred | Pixel math, general filtering, gain fields, registration and multi-frame work. Add payloads with their consumers. |
| Chart calibration | Deferred | Fit reusable transforms from separate chart images ([DESIGN](DESIGN.md#processing)). |
| Camera color / other CFAs | Deferred | Calibration beyond the matrix and as-shot balance, and X-Trans and other non-Bayer inputs. |
| Windows RAW paths | Open | Use LibRaw's wide-path API. |
| Drop the wgpu fork | Deferred | Upgrade when a release supports passthrough ([THIRD_PARTY](../THIRD_PARTY.md)). |
| CLI | Deferred | Templates with view kinds, template parameters, overwrite policy and partial failures. |
| Project-relative paths | Open | Resolve paths against the project location, with the CLI work. |
| Save/close protection | Deferred | Failure-safe saves and exports, a prompt when closing with unsaved edits, and in-flight export handling. |
| File changes | Idea | Detect changed ICC files, which are read on first use and assumed unchanged until chosen again. |
| End-to-end latency | Measured | Full-detail worker latency is 164–171 ms, with about 64 ms of CPU preview packing and 86 ms of GPU completion and readback. Profile display upload and consider device packing. |
| Memory and drawing cost | Deferred | Measure retained RAWs, export buffers, duplicate uploads across windows and scope meshes before streaming or sharing textures. |
| Evaluation performance | Deferred | Cooperative cancellation and backend tuning, driven by measured latency. |
| Lens-correction metadata | Deferred | Crop factor, correction profiles, exposure focus distance and image-to-capture geometry. |
| Capture metadata | Deferred | Time zone and maker-note passthrough, and a wider TIFF EXIF export. |
| HDR | Deferred | Define headroom and metadata, and track Windows desktop SDR white. |
| Native windows | Deferred | Pop-out parenting on Windows and macOS, pin control, and pacing and occlusion across windows. Retest hidden-window pacing and native dialogs. |
| Deflate levels 1–9 | Blocked | The TIFF encoder exposes only fast, balanced and best. |
| RawSpeed evaluation | Deferred | Measure coverage, speed and dependency cost against LibRaw. |
| LibRaw OpenMP | Deferred | Resolve runtime bundling on every platform and measure decode gains. |
| v0.1 release scope | Open | Set criteria after proof-of-concept feedback and portability checks. |
| CPU-specific builds | Deferred | Measure x86-64-v3 gains before adding variants. |

## Next image nodes

Proposed order for ordinary still-photo editing. Order is implementation
priority, not processing order. Dependencies and defaults remain proposals
until implementation.

| Order | Node work | First use | Initial scope |
|-------|-----------|-----------|---------------|
| 1 | RGB image input | Edit a JPEG or bring in a TIFF reference. | JPEG, PNG and TIFF sources with ICC, orientation, an explicit untagged-profile assumption and defined alpha. Decode into linear RGB without treating a rendered image as scene data. |
| 2 | Saturation/chroma | Make a landscape less dull or a portrait less colourful. | One global chroma adjustment with a stated colour basis and luminance and hue behaviour. Define signed and HDR handling without clipping to a display range. |
| 3 | Tone curve | Lift midtones or adjust the toe without replacing the rendering. | An editable master curve with a defined luminance or per-channel policy and black and above-range behaviour. Exposure and Sigmoid keep their jobs. |
| 4 | Masks and blend | Brighten a face or darken the sky. | A mask payload with its Blend consumer, linear and radial producers, a mask view and direct manipulation, reusing existing adjustments. Brush and colour or luminance selections follow. |
| 5 | Denoise | Clean up a high-ISO photograph. | A documented algorithm with luminance and chroma controls that behaves predictably at preview scales. Verify detail and artifacts at full resolution, and choose its input domain with it. |
| 6 | Sharpen | Restore apparent detail, including after resizing. | Landed 2026-10-13 as an unsharp mask. A threshold and the recommended placement remain. |
| 7 | Lens correction | Remove barrel distortion, dark corners and lateral colour fringes. | Load lens models apart from applying them, starting with distortion and vignetting, then lateral CA. Use capture metadata, an explicit image-to-capture mapping and applicability checks. |

Keep local edits compositional: an adjusted image and its base feed Blend,
rather than embedding masking separately into every adjustment.
