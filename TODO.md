# TODO

Open work. Deferred ideas need a concrete use or measurement before implementation.

| Work | Status | Remaining scope |
|------|--------|-----------------|
| Production pipeline validation | Follow-up | Current image quality is accepted for the user-testable proof of concept. Broader detail/highlight/color review against measured/reference evidence is not a gate for that milestone. |
| Editing UI/UX | Follow-up | Existing workflows are accepted for the proof of concept; refine them from tester feedback. |
| Display portability | Follow-up | Require correct color on Wayland, Windows and macOS. Verify native tagging, reference white, monitor/profile changes and Windows Advanced Color on/off; legacy Windows needs explicit ICC output. Current Windows/macOS paths remain unverified; document tested environments for the proof of concept. |
| SpyderX verification | Follow-up | Measure the complete display color path; not a gate for the initial user-testable proof of concept. |
| Packaging and initial proof of concept | Next | Package the existing editor, verify installation/launch and RAW-to-TIFF use in a clean environment, and document tested configurations and known limitations. |
| Build portability | Open | Verify Rust 1.95 and Windows/macOS builds and linker discovery. |
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
| Portable release artifacts | Next | CI supplies ZIPs containing the GUI and scaffold CLI. Add Linux `.tar.xz`/Flatpak and macOS `.app` packaging, plus source bundles containing submodule contents. Establish minimum supported Linux systems for artifacts built on the latest runner images, audit runtime-loaded dependencies, and smoke-test clean desktops. Choose macOS deployment target and Windows runtime packaging. |
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
| Dedicated node picker | Deferred | Replace the canvas Add node menu using the kinds' category and name metadata. |
| Lens correction | Deferred | Add lens correction after the initial user-testable proof of concept; not a gate for that milestone. |
| RawSpeed evaluation | Deferred | Evaluate [RawSpeed](https://github.com/darktable-org/rawspeed) for specific unsupported formats and decode-speed gains. Compare decoded samples/metadata and build dependencies; check which formats LibRaw's integration actually exposes. Separate decoder coverage from Drip's Bayer-only restriction. |
| LibRaw OpenMP | Deferred | Evaluate parallel RAW decoding after resolving OpenMP runtime acquisition and bundling on Linux, macOS and Windows. Check upstream platform restrictions and measure decode gains; keep OpenMP disabled for the initial proof of concept. |
| v0.1 release scope | Open | Set release criteria after proof-of-concept feedback and remaining portability/color evidence. |
| CPU-specific distribution builds | Deferred | Revisit x86-64-v3 after measuring gains. Keep Rust and native CPU settings consistent, preserve user overrides and ARM64 support, and avoid a custom build wrapper. Current builds use toolchain defaults. |
