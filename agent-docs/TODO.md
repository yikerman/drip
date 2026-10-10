# TODO

Unfinished work. Deferred ideas need a concrete use or measurement.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Packaged proof of concept | Next | Verify installation, launch and RAW-to-TIFF use on clean desktops; record supported environments and known limitations. |
| Portable artifacts | Next | Add Linux `.tar.xz`/Flatpak and macOS `.app` bundles. Choose Linux minimum systems, macOS deployment target and Windows runtime packaging; audit runtime-loaded dependencies. Preserve executable permissions, bundle submodule sources, and include build provenance/notices. |
| Build portability | Open | Verify Rust 1.95 and full native Windows/macOS builds and staging. |
| Display portability | Open | Verify Windows/macOS tagging, reference white, monitor/profile transitions and Advanced Color on/off; legacy Windows needs ICC output. Exercise NVIDIA, other compositors and HDR white. |
| Image/color validation | Follow-up | Review detail, highlights and color against measured targets; measure the display path with SpyderX. Not a proof-of-concept gate. |
| Editing UI/UX | Follow-up | Refine from tester feedback; current workflows suffice for the proof of concept. |
| Remaining frontend presentation | Open | Audit items outside the requested fixes: export-depth/intent terminology, explicit EV reference annotations, proof-mode badges, error elision, blank/stale pop-out views and distinct loading phases. |
| Bound-source declarations | Open | Extend generated declarations to immutable bind-time assets and shared/unavailable outputs; remove handwritten RAW metadata/port duplication after agreeing on the interface. |
| RAW metadata extraction | Open | Canon EOS D30 (#1307): unusable as-shot WB. Samsung GX-1L (#8115): unusable WB and zero camera matrix. Excluded from the active corpus; IDs refer to raw.pixls.us. |
| DNG matrix extraction | Open | Sigma fp 8-bit (#7280) and 14-bit (#7273): nonzero ColorMatrix1/2, but LibRaw returns zero `cam_xyz` after unpack. Excluded from the active corpus. |
| Panic-time native teardown | Open | Investigate NVIDIA/Wayland segmentation fault during unwind after the fixed image-view panic. |
| Worker startup failures | Open | Report thread-creation failures in `Worker::new` and action dispatch without losing the preview worker. |
| Evaluation scale bounds | Open | Public level 31 overflows bin2x2 scale. |
| Node ID exhaustion | Open | Loading ID `u64::MAX - 1` then adding a node overflows `Graph::insert`. |
| Sensor calibration | Open | Expose sample conventions and sensor coordinates before dark/flat correction; see [DESIGN](DESIGN.md). |
| Processing extensions | Deferred | Masks, pixel math, filtering, lens correction, gain fields, registration and multi-frame denoise/HDR/super-resolution/stitching. Add payloads with consumers. |
| Chart calibration | Deferred | Fit reusable transforms from separate chart images; see [DESIGN](DESIGN.md). |
| Camera color / other CFAs | Deferred | Calibration beyond matrix/as-shot WB; X-Trans and other non-Bayer inputs. |
| Windows RAW paths | Open | Use LibRaw's wide-path API. |
| Drop the wgpu fork | Deferred | Upgrade when a compatible release supports passthrough; see [THIRD_PARTY](../THIRD_PARTY.md). |
| CLI | Deferred | Template parameters, overwrite/collision policy and partial failures. |
| Project-relative paths | Open | Resolve against the project location alongside CLI work. |
| Save/close protection | Deferred | Failure-safe saves/exports, unsaved-close confirmation and in-flight export handling. |
| Resource changes | Idea | Detect changed input files. |
| End-to-end latency | Open | Measure preparation/upload/draw too; consider Tracy via `profiling`. |
| Memory and drawing cost | Deferred | Measure retained RAWs/export buffers, duplicate GPU uploads across windows and scope mesh copies before considering streaming TIFF/shared textures. |
| Evaluation performance | Deferred | Cooperative cancellation, full-detail ROI and backend optimization, driven by measured latency. |
| Capture metadata | Deferred | Lens, GPS, orientation, time zone and maker-note passthrough. |
| HDR | Deferred | Define headroom/metadata and track Windows desktop SDR white. |
| Native windows | Deferred | Windows/macOS parenting and pin control; cross-window pacing/occlusion. Retest hidden-window pacing and native dialogs after their fixes. |
| Undo/redo | Deferred | Revisit with editing workflows. |
| Deflate levels 1–9 | Blocked | TIFF encoder exposes only fast/balanced/best. |
| Dedicated node picker | Deferred | Replace the canvas Add node menu using kind categories/names. |
| RawSpeed evaluation | Deferred | Measure decoder coverage/speed and dependency cost; distinguish standalone support, LibRaw integration and Drip's CFA restrictions. |
| LibRaw OpenMP | Deferred | Resolve runtime acquisition/bundling on all platforms, check upstream restrictions and measure decode gains. |
| v0.1 release scope | Open | Set criteria after proof-of-concept feedback and portability/color verification. |
| CPU-specific builds | Deferred | Measure x86-64-v3 gains before adding variants; keep Rust/native settings consistent, user overrides and ARM64 support. |
