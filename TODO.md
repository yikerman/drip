# TODO

Open work. Deferred ideas need a concrete use or measurement before implementation.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Production pipeline validation | Next | Measure detail, highlights and color against references. |
| Editing UI/UX | Planned | Refine production workflows; consider a dedicated node picker. |
| Display portability | Next | Verify Wayland, macOS and Windows tagging, reference white, monitor/profile changes and Windows Advanced Color on/off. Legacy Windows needs explicit ICC output. |
| SpyderX verification | Planned | Measure the complete display path before release. |
| Packaging and v0.1 | Planned | Follow processing, workflow and color verification. |
| Build portability | Open | Verify Rust 1.92 and Windows/macOS builds and linker discovery. |
| RAW metadata extraction | Open | Canon EOS D30 (#1307): unusable as-shot WB. Samsung GX-1L (#8115): unusable WB and zero camera matrix. Both excluded from active corpus. IDs refer to raw.pixls.us. |
| DNG matrix extraction | Open | Sigma fp 8-bit (#7280) and 14-bit (#7273): nonzero ColorMatrix1/2, but LibRaw 0.22.2 returns zero `cam_xyz` after unpack. Excluded from active corpus. |
| Panic-time native teardown | Open | Investigate NVIDIA/Wayland segmentation fault during unwind after the now-fixed image-view panic. |
| Worker startup failures | Open | Report thread-creation errors in `Worker::new` and action dispatch; export startup currently panics and stops the preview worker. |
| Evaluation scale bounds | Open | Public level 31 overflows bin2x2 scale. |
| Node ID exhaustion | Open | Loading ID `u64::MAX - 1` then adding a node overflows `Graph::insert`. |
| Sensor calibration | Open | Expose sample conventions and sensor coordinates before dark/flat correction; see DESIGN. |
| Payloads and processing nodes | Deferred | Masks, pixel math, filtering, lens correction, gain fields, registration and multi-frame denoise/HDR/super-resolution/stitching. Add payloads with consumers. |
| Chart calibration | Deferred | Fit reusable transforms from separate chart images; see DESIGN. |
| Camera color / other CFAs | Deferred | Calibration beyond matrix/as-shot WB; X-Trans and other non-Bayer inputs. |
| Minimal LibRaw build | Planned | Vendor only needed decoder features. |
| Windows RAW paths | Open | Use LibRaw's wide-path API. |
| Drop the wgpu fork | Deferred | Upgrade when a compatible release supports passthrough; see THIRD_PARTY. |
| CLI | Deferred | Template inputs, overwrite/collision policy and partial failures. |
| Project-relative paths | Open | Resolve against the project location alongside CLI work. |
| Save/close protection | Deferred | Failure-safe saves/exports, unsaved edits and in-flight export handling. |
| Resource changes | Idea | Detect changed input files; invalidation is manual today. |
| End-to-end latency | Open | Include preparation/upload/draw; consider Tracy via `profiling`. |
| Memory and drawing cost | Deferred | Measure retained RAWs/export buffers, duplicate GPU uploads across windows and scope mesh copies. Consider streaming TIFF and shared textures only if justified. |
| Evaluation performance | Deferred | Cooperative cancellation, full-detail ROI and GPU execution, driven by measured latency. |
| Capture metadata | Deferred | Lens, GPS, orientation, time zone and maker-note passthrough. |
| HDR | Deferred | Define headroom/metadata and track Windows desktop SDR white. |
| Native windows | Deferred | Windows/macOS parenting and pin control; cross-window pacing/occlusion. |
| Undo/redo | Deferred | Revisit with editing workflows. |
| Deflate levels 1–9 | Blocked | TIFF encoder exposes only fast/balanced/best. |
| AGPL variant | Open | Confirm `-or-later` versus `-only`. |
