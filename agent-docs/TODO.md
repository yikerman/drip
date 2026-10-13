# TODO

Unfinished work. Deferred ideas need a concrete use or measurement.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Explicit compute model | In progress | Steps 3 to 8 of the [migration](COMPUTE_MODEL.md#migration-in-the-drip-crate): `warp` and `resample` replacing `RgbField`, keys with durability and a device budget, `foreign` with a disk tier, regions, `reduce`, `stencil` and `meta`. Step 4 shows that a Sigmoid-only edit recomputes only the fused tail and decides whether a source keeps its device upload across demands, which needs ownership, lifetime and a byte budget. Map bodies read only `Rgb`. Other payloads and maps with several mapped inputs wait for a node that needs them. |
| Raster runtime identity | Follow-up | A raster's device representation belongs to the runtime that made it, and `Raster::device` does not check that a later caller uses the same runtime. Check or key it once the store outlives one evaluator (step 4). `Statistics` counts per runtime, so concurrent evaluations on one runtime mix their counts. |
| Remaining simplifications | Follow-up | Revisit shared preparation/kernel context, colorimetry functions and derived shader matrices, evaluation-loop consolidation and preview-context ownership separately. Presentation-only edits need a drawing/preparation path. Batched scope readback needs performance evidence. |
| Packaged proof of concept | Next | Verify saved edits and full-detail RAW-to-TIFF use, plus installation/launch on clean desktops. Record supported environments and known limitations. |
| Portable artifacts | Next | Add Linux `.tar.xz`/Flatpak and macOS `.app` bundles. Choose Linux minimum systems, macOS deployment target and Windows runtime packaging. Audit runtime-loaded dependencies. Preserve executable permissions, bundle submodule sources, and include build provenance/notices. |
| Build portability | Open | Verify Rust 1.95 and full native Windows/macOS builds and staging. |
| Display portability | Open | Verify Windows/macOS tagging, reference white, monitor/profile transitions and Advanced Color on/off. Legacy Windows needs ICC output. Exercise NVIDIA, other compositors and HDR white. |
| Image/color validation | Follow-up | Review detail, highlights and color against measured targets. Measure the display path with SpyderX. Not a proof-of-concept gate. |
| Scene-illuminant white balance | Integration test pending | Implemented. A user check verifies it: saved overrides reopen unchanged, and full-detail preview and export agree. |
| Photo orientation | Integration test pending | Implemented. A user check verifies it: preview, override, project reopening and TIFF orientation. |
| Auto exposure | Integration test pending | Implemented on the worker queue. A user check verifies it: real-photo response, repeated clicks, saved EV and preview/export agreement. |
| Perspective Transform view | Integration test pending | Tests cover preparation on the compute runtime and the frame geometry. A desktop session verifies the handles, Keep inside and Fit. |
| White-balance controls | Follow-up | Add temperature/tint and neutral sampling to the CAT16 controls when needed. Initial controls accept source xy. |
| Partial chromatic adaptation | Deferred | Define whether the aim is manual mood preservation or observer-appearance prediction before adding adaptation-degree controls. Compare generalized/two-step CAT16 and WGM against photographic examples. |
| Exposure normalization | Deferred | Neither manual exposure nor mean-to-18% metering calibrates an 18% scene reflector. Automatic baseline handling and cross-camera exposure matching need a defined convention and validation. |
| Output-gamut rendering | Deferred | Revisit explicit target-gamut rendering only when image evidence warrants it. Distinguish source compression from output mapping. |
| CJK text | Deferred | Embedded fonts cover Latin, Greek and Cyrillic, and CJK names and paths render as `?`. Prefer a system-font fallback at runtime over embedding Source Han Sans (several MB per region and weight). |
| Editing UI/UX | Follow-up | Refine from tester feedback. |
| Remaining frontend presentation | Open | Export-depth/intent terminology, explicit EV reference annotations, proof-mode badges, error elision and blank/stale pop-out views. |
| RAW metadata extraction | Open | Canon EOS D30 (#1307): unusable as-shot WB. Samsung GX-1L (#8115): unusable WB and zero camera matrix. Excluded from the active corpus. IDs refer to raw.pixls.us. |
| DNG matrix extraction | Open | Sigma fp 8-bit (#7280) and 14-bit (#7273): nonzero ColorMatrix1/2, but LibRaw returns zero `cam_xyz` after unpack. Excluded from the active corpus. |
| Panic-time native teardown | Open | Investigate the NVIDIA/Wayland segmentation fault during unwind after the fixed image-view panic. |
| Worker startup failures | Open | Report thread-creation failures in `Worker::new` without losing the worker. |
| Sensor calibration | Open | Expose sample conventions and sensor coordinates before dark/flat correction. See [DESIGN](DESIGN.md#processing). |
| Everyday image nodes | Proposed | Implement in the order under [Next image nodes](#next-image-nodes). Not part of the packaged proof-of-concept gate. |
| Geometry template | Follow-up | `templates/straighten-and-resize.drip-template` is not listed in `drip-gui/src/templates.rs`, so it is neither built in nor checked against its re-serialization. List it, or regenerate it when step 3 replaces the field nodes. |
| Geometry extensions | Open | Single-channel geometry for masks, one map shared by an image and its mask, resampling onto another image's grid for registration, EWA footprints, deferred nonlinear value operations, and a citable derivation of repeated-resampling loss. See [FIELD](FIELD.md#open-questions). |
| Later processing extensions | Deferred | Pixel math, general filtering, gain fields, registration and multi-frame denoise/HDR/super-resolution/stitching. Add payloads with consumers. |
| Chart calibration | Deferred | Fit reusable transforms from separate chart images. See [DESIGN](DESIGN.md#processing). |
| Camera color / other CFAs | Deferred | Calibration beyond matrix/as-shot WB. X-Trans and other non-Bayer inputs. |
| Windows RAW paths | Open | Use LibRaw's wide-path API. |
| Drop the wgpu fork | Deferred | Upgrade when a compatible release supports passthrough. See [THIRD_PARTY](../THIRD_PARTY.md). |
| CLI | Deferred | Loading GUI-saved templates that contain view kinds, template parameters, overwrite/collision policy and partial failures. |
| Project-relative paths | Open | Resolve against the project location alongside CLI work. |
| Save/close protection | Deferred | Failure-safe saves/exports, confirmation when closing the app with unsaved edits, and in-flight export handling. |
| File changes | Idea | Detect changed RAW and ICC files. Nodes assume them unchanged until chosen again. |
| End-to-end latency | Measured | Device scopes brought full-detail worker latency to 164–171 ms. Profile display upload/draw. Remaining worker costs include about 64 ms of CPU preview packing and an 86 ms GPU-completion/readback span. Consider device RGBA packing, and keep LittleCMS proofing on the CPU. |
| Memory and drawing cost | Deferred | Measure retained RAWs/export buffers, duplicate GPU uploads across windows and scope mesh copies before considering streaming TIFF/shared textures. |
| Evaluation performance | Deferred | Cooperative cancellation and backend optimization, driven by measured latency. Full-detail regions are step 6 of the compute model. |
| Lens-correction metadata | Deferred | Resolve crop factor and correction profiles. Extract exposure focus distance where available (lens minimum focus distance is distinct). Preserve image-to-capture geometry. |
| Capture metadata | Deferred | Time zone and private maker-note passthrough. Extend TIFF EXIF export separately. |
| HDR | Deferred | Define headroom/metadata and track Windows desktop SDR white. |
| Native windows | Deferred | Windows/macOS parenting and pin control, cross-window pacing/occlusion. Retest hidden-window pacing and native dialogs after their fixes. |
| Deflate levels 1–9 | Blocked | The TIFF encoder exposes only fast/balanced/best. |
| RawSpeed evaluation | Deferred | Measure decoder coverage/speed and dependency cost. Distinguish standalone support, LibRaw integration and Drip's CFA restrictions. |
| LibRaw OpenMP | Deferred | Resolve runtime acquisition/bundling on all platforms, check upstream restrictions and measure decode gains. |
| v0.1 release scope | Open | Set criteria after proof-of-concept feedback and portability/color verification. |
| CPU-specific builds | Deferred | Measure x86-64-v3 gains before adding variants. Keep Rust/native settings consistent, user overrides and ARM64 support. |

## Next image nodes

Proposed order for ordinary still-photo editing. Order is implementation
priority, not processing order. Dependencies and defaults remain proposals
until implementation.

| Order | Node work | First use | Initial scope |
|-------|-----------|-----------|---------------|
| 1 | RGB image input | Edit a JPEG or bring in a TIFF reference. | RGB JPEG/PNG/TIFF sources with ICC and orientation handling, an explicit untagged-profile assumption and defined alpha handling. Decode into documented linear RGB coordinates without treating an already rendered image as unrendered scene data. |
| 2 | Saturation/chroma | Make a landscape less dull or a portrait less colourful. | One global chroma adjustment with stated colour basis and luminance/hue behavior. Define signed/HDR handling, with no implicit clipping into a display range. |
| 3 | Tone curve | Lift midtones or adjust the toe without replacing the whole rendering. | Editable master curve with a defined luminance/per-channel policy, black and above-range behavior. Reuse existing exposure and sigmoid controls for their current jobs. |
| 4 | Masks and blend | Brighten a face or darken the sky. | Add a mask payload with its Blend consumer and linear/radial mask producers together. Blend base and adjusted RGB with explicit spatial, colour and weight contracts. Reuse existing adjustment nodes. Add a mask view and direct manipulation. Brush and colour/luminance selections follow later. |
| 5 | Denoise | Clean up a high-ISO photograph. | A documented starting algorithm with useful luminance/chroma controls and predictable behavior at preview scales. Verify detail retention and artifacts at full resolution. Choose the input domain with the algorithm. |
| 6 | Sharpen | Restore apparent detail, including after delivery resizing. | Amount, radius and threshold controls with declared radius units and border behavior. Document recommended placement without enforcing it as a graph restriction. |
| 7 | Lens correction | Remove barrel distortion, dark corners and lateral colour fringes. | Separate lens-model loading from its application. Start with distortion and vignetting, then lateral CA. Use capture metadata, explicit image-to-capture mapping and model-applicability checks. Distortion and lateral CA are per-channel coordinate maps, and vignetting is a per-pixel gain. |

Keep local edits compositional: an adjusted image and its base feed Blend,
rather than embedding masking separately into every adjustment.
