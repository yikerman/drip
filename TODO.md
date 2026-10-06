# TODO

Unimplemented work only. Deferred work is not a commitment; revisit prototype
robustness and performance when usage or measurements justify it.

| Idea | Status | Scope / reason to revisit |
|------|--------|---------------------------|
| Production pipeline validation | Next | Cross-camera contract tests are in place; judge detail, highlights and color against measured/reference evidence. |
| Broader RAW support / correct unpacking | Open | Investigate metadata extraction for Canon EOS D30 (`canon-eos-d30.crw`, raw.pixls.us #1307): unusable as-shot WB; Samsung GX-1L (`samsung-gx-1l.pef`, #8115): unusable WB and zero camera matrix. Samples removed from the active corpus until supported. |
| DNG camera matrix unpacking | Open | Sigma fp 8-bit (`sigma-fp.dng`, raw.pixls.us #7280) and 14-bit (`sigma-fp-14bit-3-2.dng`, #7273) contain nonzero ColorMatrix1/2 tags, but LibRaw 0.22.2 returns zero `cam_xyz` after unpack. Investigate correct extraction; samples removed from the active corpus. |
| Panic-time native teardown | Open | User observed a segmentation fault after the image-view Rust panic on NVIDIA/Wayland. The triggering panic is fixed; investigate unwind-time surface/connection destruction separately. Native failure not reproduced by headless tests. |
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
| Windows/macOS pop-outs | Deferred | Parenting and a pin control where the WM provides none. |
| Cross-window pacing / occlusion | Deferred | Revisit hidden-window Fifo stalls if they matter in use. |
| Undo/redo | Deferred | Not needed for current prototype use. |
| Deflate levels 1–9 | Blocked | TIFF encoder only exposes fast/balanced/best. |
