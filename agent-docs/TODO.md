# TODO

Unfinished work. Deferred ideas need a concrete use or measurement.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Remaining simplifications | Follow-up | Revisit shared preparation/kernel context, colorimetry helpers and derived shader matrices, evaluation-loop consolidation, category declarations and preview-context ownership separately. Presentation-only edits need a drawing/preparation path; source device residency and batched scope readback need ownership and performance evidence. |
| Packaged proof of concept | Next | No additional processing stage identified in the post-CAT16 review. Verify saved edits and full-detail RAW-to-TIFF use, plus installation/launch on clean desktops; record supported environments and known limitations. |
| Portable artifacts | Next | Add Linux `.tar.xz`/Flatpak and macOS `.app` bundles. Choose Linux minimum systems, macOS deployment target and Windows runtime packaging; audit runtime-loaded dependencies. Preserve executable permissions, bundle submodule sources, and include build provenance/notices. |
| Build portability | Open | Verify Rust 1.95 and full native Windows/macOS builds and staging. |
| Display portability | Open | Verify Windows/macOS tagging, reference white, monitor/profile transitions and Advanced Color on/off; legacy Windows needs ICC output. Exercise NVIDIA, other compositors and HDR white. |
| Image/color validation | Follow-up | Review detail, highlights and color against measured targets; measure the display path with SpyderX. Not a proof-of-concept gate. |
| Scene-illuminant white balance | Integration checks remaining | Implemented. Confirm saved overrides and full-detail preview/export agreement. |
| White-balance controls | Follow-up | Add temperature/tint and neutral sampling when needed; initial controls accept source xy. |
| Partial chromatic adaptation | Deferred | Define whether the aim is manual mood preservation or observer-appearance prediction before adding adaptation-degree controls. Compare generalized/two-step CAT16 and WGM against photographic examples; initial WB uses full adaptation. |
| Exposure normalization | Deferred | Manual exposure and whole-image mean-to-18% metering are available. Neither calibrates an 18% scene reflector; automatic baseline handling and cross-camera exposure matching need a defined convention and validation. |
| Photo orientation | Integration test pending | Metadata-driven RGB orientation, parameter override and template wiring are implemented. User will verify preview, override, project reopening and TIFF orientation interactively. |
| Auto exposure | Integration test pending | Runs on the worker queue. User will check real-photo response, repeated clicks, saved EV and preview/export agreement. |
| Output-gamut rendering | Deferred | LittleCMS plus selected rendering intent is accepted for the prototype. Revisit explicit target-gamut rendering only when image evidence warrants it; distinguish source compression from output mapping. |
| CJK text | Deferred | Embedded fonts cover Latin, Greek and Cyrillic; CJK names and paths render as `?`. Prefer a system-font fallback at runtime over embedding Source Han Sans (several MB per region and weight). |
| Editing UI/UX | Follow-up | Refine from tester feedback; current workflows suffice for the proof of concept. |
| Remaining frontend presentation | Open | Audit items outside the requested fixes: export-depth/intent terminology, explicit EV reference annotations, proof-mode badges, error elision and blank/stale pop-out views. |
| RAW metadata extraction | Open | Canon EOS D30 (#1307): unusable as-shot WB. Samsung GX-1L (#8115): unusable WB and zero camera matrix. Excluded from the active corpus; IDs refer to raw.pixls.us. |
| DNG matrix extraction | Open | Sigma fp 8-bit (#7280) and 14-bit (#7273): nonzero ColorMatrix1/2, but LibRaw returns zero `cam_xyz` after unpack. Excluded from the active corpus. |
| Panic-time native teardown | Open | Investigate NVIDIA/Wayland segmentation fault during unwind after the fixed image-view panic. |
| Worker startup failures | Open | Report thread-creation failures in `Worker::new` without losing the worker. |
| Sensor calibration | Open | Expose sample conventions and sensor coordinates before dark/flat correction; see [DESIGN](DESIGN.md). |
| Everyday image nodes | Proposed | Implement in the order below under [Next image nodes](#next-image-nodes). This is a proposed follow-up, not an expansion of the packaged proof-of-concept gate. |
| Continuous-geometry template | Blocked | Add `templates/` file demonstrating Interpolate → Lens correction/Perspective Transform → Sample, with previews after Sample, once those nodes exist ([FIELD](FIELD.md)). |
| Field extensions | Open | Later: `MonoField` with masks, a `Chain` payload shared by an image and its mask, Sample onto another image's grid for registration, EWA footprints, deferred nonlinear value operations, and a citable derivation of repeated-resampling loss. See [FIELD](FIELD.md#open-questions). |
| Later processing extensions | Deferred | Pixel math, general filtering, gain fields, registration and multi-frame denoise/HDR/super-resolution/stitching. Add payloads with consumers. |
| Chart calibration | Deferred | Fit reusable transforms from separate chart images; see [DESIGN](DESIGN.md). |
| Camera color / other CFAs | Deferred | Calibration beyond matrix/as-shot WB; X-Trans and other non-Bayer inputs. |
| Windows RAW paths | Open | Use LibRaw's wide-path API. |
| Drop the wgpu fork | Deferred | Upgrade when a compatible release supports passthrough; see [THIRD_PARTY](../THIRD_PARTY.md). |
| CLI | Deferred | Loading GUI-saved templates that contain view kinds; template parameters, overwrite/collision policy and partial failures. |
| Project-relative paths | Open | Resolve against the project location alongside CLI work. |
| Save/close protection | Deferred | Failure-safe saves/exports, confirmation when closing the app with unsaved edits (New and Open ask) and in-flight export handling. |
| File changes | Idea | Detect changed RAW and ICC files; nodes assume them unchanged until chosen again. |
| End-to-end latency | Measured | Device scopes reduce full-detail worker latency to 164–171 ms. Profile display upload/draw; remaining worker costs include about 64 ms of CPU preview packing and an 86 ms GPU-completion/readback span. Consider device RGBA packing; keep LittleCMS proofing on CPU. |
| Memory and drawing cost | Deferred | Measure retained RAWs/export buffers, duplicate GPU uploads across windows and scope mesh copies before considering streaming TIFF/shared textures. |
| Evaluation performance | Deferred | Cooperative cancellation, full-detail ROI and backend optimization, driven by measured latency. |
| Lens-correction metadata | Deferred | Common lens identifiers, focal/aperture data and sensor geometry are retained. Resolve crop factor and correction profiles; extract exposure focus distance where available (lens minimum focus distance is distinct). Preserve image-to-capture geometry. |
| Capture metadata | Deferred | Common capture/lens/GPS/calibration fields are available to nodes. Time zone and private maker-note passthrough remain unimplemented; extend TIFF EXIF export separately. |
| HDR | Deferred | Define headroom/metadata and track Windows desktop SDR white. |
| Native windows | Deferred | Windows/macOS parenting and pin control; cross-window pacing/occlusion. Retest hidden-window pacing and native dialogs after their fixes. |
| Deflate levels 1–9 | Blocked | TIFF encoder exposes only fast/balanced/best. |
| RawSpeed evaluation | Deferred | Measure decoder coverage/speed and dependency cost; distinguish standalone support, LibRaw integration and Drip's CFA restrictions. |
| LibRaw OpenMP | Deferred | Resolve runtime acquisition/bundling on all platforms, check upstream restrictions and measure decode gains. |
| v0.1 release scope | Open | Set criteria after proof-of-concept feedback and portability/color verification. |
| CPU-specific builds | Deferred | Measure x86-64-v3 gains before adding variants; keep Rust/native settings consistent, user overrides and ARM64 support. |

## Next image nodes

Proposed order for ordinary still-photo editing, based on the current registry.
Exposure, CAT16 white balance, orientation, sigmoid contrast and TIFF export
already exist. Node order here is implementation priority, not mandatory
processing order. The first three nodes are refined below; dependency and
filter defaults remain proposals until implementation.

| Order | Node work | First use | Initial scope |
|-------|-----------|-----------|---------------|
| 1 | Fields, Interpolate and Sample (implemented) | Deliver a photograph with a 2048-pixel long edge. | `RgbField`, Interpolate and Sample per [FIELD](FIELD.md): a factor or long-edge size, five filters scaled by the footprint (Lanczos-3 default), signed/HDR linear RGB. Footprints clamp at 16 source samples; strong reductions need a pyramid or EWA. |
| 2 | Perspective Transform | Crop, level a horizon or straighten a building. | `RgbField -> RgbField`: one four-point quad plus aspect covers crop, rotation and keystone as one projective map stage ([FIELD](FIELD.md#perspective-transform)). Its own editing view with Frame and Perspective tools, Keep inside and Fit. |
| 3 | JPEG export (implemented) | Share an edit without a TIFF conversion step. | Uses jpeg-encoder 0.7.1, shared output-profile conversion and node actions. sRGB/quality 95/4:4:4 defaults, embedded ICC, optional capture EXIF and selectable averaged chroma subsampling. |
| 4 | RGB image input | Edit a JPEG or bring in a TIFF reference. | RGB JPEG/PNG/TIFF sources with ICC and orientation handling, an explicit untagged-profile assumption and defined alpha handling. Decode into documented linear RGB coordinates without treating an already rendered image as unrendered scene data. |
| 5 | Saturation/chroma | Make a landscape less dull or a portrait less colourful. | One global chroma adjustment with stated colour basis and luminance/hue behavior. Define signed/HDR handling; no implicit clipping into a display range. |
| 6 | Tone curve | Lift midtones or adjust the toe without replacing the whole rendering. | Editable master curve with a defined luminance/per-channel policy, black and above-range behavior. Reuse existing exposure and sigmoid controls for their current jobs. |
| 7 | Masks and blend | Brighten a face or darken the sky. | Add a Mask payload with its Blend consumer and linear/radial mask producers together. Blend base and adjusted RGB with explicit spatial, colour and weight contracts. Reuse existing adjustment nodes; add a mask view and direct manipulation. Brush and colour/luminance selections follow later. |
| 8 | Denoise | Clean up a high-ISO photograph. | A documented starting algorithm with useful luminance/chroma controls and predictable behavior at preview scales. Verify detail retention and artifacts at full resolution; choose the input domain with the algorithm. |
| 9 | Sharpen | Restore apparent detail, including after delivery resizing. | Amount, radius and threshold controls with declared radius units and border behavior. Document recommended placement without enforcing it as a graph restriction. |
| 10 | Lens correction | Remove barrel distortion, dark corners and lateral colour fringes. | Separate lens-model loading from its application. Start with distortion and vignetting, then lateral CA; use capture metadata and explicit image-to-capture mapping, and model-applicability checks. Distortion and lateral CA are per-channel field map stages; vignetting is a per-pixel gain. |

Alongside these nodes, add temperature/tint and neutral sampling to the existing
CAT16 controls; this is the white-balance-controls item above, not another
adaptation node. Keep local edits compositional: an adjusted image and its base
feed Blend, rather than embedding masking separately into every adjustment.

### First implementation sequence

JPEG export is implemented. Geometry follows the sequence in
[FIELD](FIELD.md#implementation-sequence); summarized:

1. **JPEG export (done):** registered an encoding action beside TIFF using
   `jpeg-encoder`, shared output conversion and capture-tag writing. Independent
   JPEG/EXIF decoder tests cover pixels, profiles, capture tags, averaged chroma,
   dimension limits and export after project persistence.
2. **Geometry foundations (done):** `RgbDesc::full` with the pixel-centre convention,
   then pass-through outputs in `#[node]`, then shared `#[cube]` filters (box,
   triangle, Mitchell–Netravali, Lanczos-3). Retain existing serialized RGB
   reduction behavior.
3. **Resample (done):** `RgbField`, Interpolate and Sample with identity maps; the
   footprint-scaled kernel replaces a separable Downsample. Test constant
   preservation, identity within tolerance, odd extents, noninteger factors,
   signed/HDR samples, zone-plate aliasing and preview/export geometry on CPU
   and WGPU.
4. **Perspective Transform:** projective map stage composed on the host and
   checked by the contract; reject undefined transforms, not unconventional
   framing. Points outside the source or through a projective pole are black.
   Then the editing view with Frame and Perspective tools, Keep inside and
   Fit. Test exact integer crop, landmark mappings, borders, minification and
   preview/export framing.

Research (2026-10-12):

- [jpeg-encoder 0.7.1 API](https://docs.rs/jpeg-encoder/0.7.1/jpeg_encoder/struct.Encoder.html)
  and [source](https://docs.rs/crate/jpeg-encoder/0.7.1/source/src/encoder.rs):
  direct RGB encoding, selectable subsampling, ICC chunking and EXIF embedding.
- [image JPEG encoder](https://docs.rs/image/0.25.10/image/codecs/jpeg/struct.JpegEncoder.html):
  viable quality/ICC/EXIF alternative, without an exposed subsampling selector.
- [fast_image_resize filters](https://docs.rs/fast_image_resize/6.0.0/src/fast_image_resize/convolution/filters.rs.html):
  filter families and custom support; CPU resizer reference, not a replacement
  for Drip's shared backend kernels.
- [vkdt crop shader](https://github.com/hanatos/vkdt/blob/f0be6383fbf80bc58d58b991ded28bf3da01c1c3/src/pipe/modules/crop/main.comp):
  combines crop, rotation and perspective in one inverse-map pass.
- [OpenCV geometric transformations](https://docs.opencv.org/4.13.0/da/d54/group__imgproc__transform.html)
  and [PBRT image filtering](https://pbr-book.org/4ed/Textures_and_Materials/Image_Texture):
  inverse mapping and footprint-dependent filtering references.
- darktable, vkdt, RawTherapee, OpenImageIO, Nuke and Blender resampling at
  pinned revisions, and the filtering literature: see
  [FIELD](FIELD.md#prior-art).
