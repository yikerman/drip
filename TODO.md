# TODO

Unimplemented work only. Deferred work is not a commitment; revisit prototype
robustness and performance when usage or measurements justify it.

| Idea | Status | Scope / reason to revisit |
|------|--------|---------------------------|
| Production pipeline validation | Next | Judge detail, highlights and color on varied photos/cameras beyond the Sony fixture. |
| Editing UI/UX | Planned | Refine workflows around the production pipeline. |
| Dedicated node picker | Deferred | Replace the canvas Add node menu using the kinds' category and name metadata. |
| Display portability | Next | Verify current output on NVIDIA, other compositors and macOS; Windows remains best effort and untested. |
| SpyderX verification | Planned | Measure the complete display color path before release. |
| Packaging and v0.1 | Planned | Follow processing, workflow and color verification. |
| Drop the wgpu fork | Deferred | Upgrade when a compatible release supports passthrough; see THIRD_PARTY.md. |
| Static minimal LibRaw build | Planned | Vendor only needed decoder features. |
| CLI batch frontend | Deferred | Template inputs, overwrite/collision policy and partial failures. |
| Project-relative paths | Open | Resolve against the project location alongside CLI work. |
| Windows Unicode RAW paths | Open | Use LibRaw's wide-path entry point. |
| Evaluation scale bounds | Open | Public level 31 overflows bin2x2 scale; GUI levels do not reach it. |
| AGPL variant | Open | Confirm `-or-later` versus `-only`. |
| End-to-end latency profiling | Open | Include preparation/upload/draw; Tracy via `profiling` is a candidate. |
| Failure-safe saves/exports | Deferred | Protect existing destinations on write failure. |
| Close protection | Deferred | Unsaved edits and completion of in-flight exports. |
| Resource memory / streamed TIFF | Deferred | Revisit retained RAWs and export buffers if costly. |
| Changed input files | Idea | Detect changes; invalidation is currently manual. |
| Cooperative cancellation | Deferred | Only if obsolete evaluation delays current edits noticeably. |
| GPU computation | Deferred | Require whole-node gains and one kernel source across backends. |
| Full-detail region of interest | Deferred | Efficient 1:1 viewing without whole-frame processing. |
| Camera color / other CFAs | Deferred | Calibration beyond matrix/as-shot WB; X-Trans and other non-Bayer inputs. |
| Full EXIF passthrough | Deferred | Lens, GPS, orientation, time zone and maker notes. |
| HDR / Windows SDR white | Deferred | Define headroom and metadata; track desktop SDR white on HDR displays. |
| Soft-proofing | Deferred | Preview the intended output medium. |
| Windows/macOS pop-outs | Deferred | Parenting and a pin control where the WM provides none. |
| Cross-window pacing / occlusion | Deferred | Revisit hidden-window Fifo stalls if they matter in use. |
| Undo/redo | Deferred | Not needed for current prototype use. |
| Deflate levels 1–9 | Blocked | TIFF encoder only exposes fast/balanced/best. |
