# TODO

Unfinished work. Deferred ideas need a concrete use or measurement.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Remaining simplifications | Follow-up | Mechanical cleanup is implemented. Revisit shared preparation/kernel context, colorimetry helpers and derived shader matrices, evaluation-loop consolidation, category declarations and preview-context ownership separately. Presentation-only edits need a drawing/preparation path; source device residency and batched scope readback need ownership and performance evidence. Keep current validation boundaries. Timing/zoom extraction and native-title state were left out because they add plumbing or state rather than strictly reducing complexity. |
| Packaged proof of concept | Next | No additional processing stage identified in the post-CAT16 review. Verify saved edits and full-detail RAW-to-TIFF use, plus installation/launch on clean desktops; record supported environments and known limitations. |
| Portable artifacts | Next | Add Linux `.tar.xz`/Flatpak and macOS `.app` bundles. Choose Linux minimum systems, macOS deployment target and Windows runtime packaging; audit runtime-loaded dependencies. Preserve executable permissions, bundle submodule sources, and include build provenance/notices. |
| Build portability | Open | Verify Rust 1.95 and full native Windows/macOS builds and staging. |
| Display portability | Open | Verify Windows/macOS tagging, reference white, monitor/profile transitions and Advanced Color on/off; legacy Windows needs ICC output. Exercise NVIDIA, other compositors and HDR white. |
| Image/color validation | Follow-up | Review detail, highlights and color against measured targets; measure the display path with SpyderX. Not a proof-of-concept gate. |
| Scene-illuminant white balance | Integration checks remaining | CAT16, metadata-derived As shot, custom source xy and RAW/template integration are implemented; user Vulkan GUI logs show successful evaluation. Confirm saved overrides and full-detail preview/export agreement. |
| White-balance controls | Follow-up | Add temperature/tint and neutral sampling when needed; initial controls accept source xy. |
| Partial chromatic adaptation | Deferred | Define whether the aim is manual mood preservation or observer-appearance prediction before adding adaptation-degree controls. Compare generalized/two-step CAT16 and WGM against photographic examples; initial WB uses full adaptation. |
| Exposure normalization | Deferred | Manual exposure and whole-image mean-to-18% metering are available. Neither calibrates an 18% scene reflector; automatic baseline handling and cross-camera exposure matching need a defined convention and validation. |
| Photo orientation | Integration test pending | Metadata-driven RGB orientation, parameter override and template wiring are implemented. User will verify preview, override, project reopening and TIFF orientation interactively. |
| Auto exposure | Integration test pending | Runs on the worker queue. User will check real-photo response, repeated clicks, saved EV and preview/export agreement. |
| Output-gamut rendering | Deferred | LittleCMS plus selected rendering intent is accepted for the prototype. Revisit explicit target-gamut rendering only when image evidence warrants it; distinguish source compression from output mapping. |
| Editing UI/UX | Follow-up | Refine from tester feedback; current workflows suffice for the proof of concept. |
| Wire readability | Open | Compare rounded orthogonal paths with capped Bézier handles; backward links and fan-out remain crowded. Consider obstacle avoidance and hover emphasis. Keep hit testing on the rendered path. |
| Remaining frontend presentation | Open | Audit items outside the requested fixes: export-depth/intent terminology, explicit EV reference annotations, proof-mode badges, error elision, blank/stale pop-out views and distinct loading phases. |
| RAW metadata extraction | Open | Canon EOS D30 (#1307): unusable as-shot WB. Samsung GX-1L (#8115): unusable WB and zero camera matrix. Excluded from the active corpus; IDs refer to raw.pixls.us. |
| DNG matrix extraction | Open | Sigma fp 8-bit (#7280) and 14-bit (#7273): nonzero ColorMatrix1/2, but LibRaw returns zero `cam_xyz` after unpack. Excluded from the active corpus. |
| Panic-time native teardown | Open | Investigate NVIDIA/Wayland segmentation fault during unwind after the fixed image-view panic. |
| Parallel device tests | Open | The GUI ignored-test process exited with SIGSEGV under WGPU on 2026-10-11 before reporting individual results. All 16 workspace device tests pass serially; isolate the native failure before attributing it to runtime startup or teardown. |
| Worker startup failures | Open | Report thread-creation failures in `Worker::new` without losing the worker. |
| Sensor calibration | Open | Expose sample conventions and sensor coordinates before dark/flat correction; see [DESIGN](DESIGN.md). |
| Processing extensions | Deferred | Masks, pixel math, filtering, lens correction, gain fields, registration and multi-frame denoise/HDR/super-resolution/stitching. Add payloads with consumers. |
| Chart calibration | Deferred | Fit reusable transforms from separate chart images; see [DESIGN](DESIGN.md). |
| Camera color / other CFAs | Deferred | Calibration beyond matrix/as-shot WB; X-Trans and other non-Bayer inputs. |
| Windows RAW paths | Open | Use LibRaw's wide-path API. |
| Drop the wgpu fork | Deferred | Upgrade when a compatible release supports passthrough; see [THIRD_PARTY](../THIRD_PARTY.md). |
| CLI | Deferred | Template parameters, overwrite/collision policy and partial failures. |
| Project-relative paths | Open | Resolve against the project location alongside CLI work. |
| Save/close protection | Deferred | Failure-safe saves/exports, unsaved-close confirmation and in-flight export handling. |
| File changes | Idea | Detect changed RAW and ICC files instead of a manual Reload files. |
| End-to-end latency | Measured | Device scopes reduce full-detail worker latency to 164–171 ms. Profile display upload/draw; remaining worker costs include about 64 ms of CPU preview packing and an 86 ms GPU-completion/readback span. Consider device RGBA packing; keep LittleCMS proofing on CPU. |
| Memory and drawing cost | Deferred | Measure retained RAWs/export buffers, duplicate GPU uploads across windows and scope mesh copies before considering streaming TIFF/shared textures. |
| Evaluation performance | Deferred | Cooperative cancellation, full-detail ROI and backend optimization, driven by measured latency. |
| Lens-correction metadata | Deferred | Common lens identifiers, focal/aperture data and sensor geometry are retained. Resolve crop factor and correction profiles; extract exposure focus distance where available (lens minimum focus distance is distinct). Preserve image-to-capture geometry. |
| Capture metadata | Deferred | Common capture/lens/GPS/calibration fields are available to nodes. Time zone and private maker-note passthrough remain unimplemented; extend TIFF EXIF export separately. |
| HDR | Deferred | Define headroom/metadata and track Windows desktop SDR white. |
| Native windows | Deferred | Windows/macOS parenting and pin control; cross-window pacing/occlusion. Retest hidden-window pacing and native dialogs after their fixes. |
| Undo/redo | Deferred | Revisit with editing workflows. |
| Deflate levels 1–9 | Blocked | TIFF encoder exposes only fast/balanced/best. |
| Dedicated node picker | Deferred | Replace the canvas Add node menu using kind categories/names. |
| RawSpeed evaluation | Deferred | Measure decoder coverage/speed and dependency cost; distinguish standalone support, LibRaw integration and Drip's CFA restrictions. |
| LibRaw OpenMP | Deferred | Resolve runtime acquisition/bundling on all platforms, check upstream restrictions and measure decode gains. |
| v0.1 release scope | Open | Set criteria after proof-of-concept feedback and portability/color verification. |
| CPU-specific builds | Deferred | Measure x86-64-v3 gains before adding variants; keep Rust/native settings consistent, user overrides and ARM64 support. |
