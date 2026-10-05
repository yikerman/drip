# TODO

Unimplemented work only. “Deferred” includes user-postponed work; it is not a
commitment to implement. Current costs and prototype robustness are acceptable
for now. Revisit performance work when measurements or usage justify it.

| Idea | Status | Reason to revisit / scope |
|------|--------|---------------------------|
| Static minimal LibRaw build | Planned | Vendor the needed decoder features for controlled builds. |
| CLI batch frontend | Deferred | Apply template inputs; define overwrite, destination collisions and partial-failure behavior. |
| Project-relative paths | Open | Resolve paths against the project location, alongside CLI work. |
| Windows Unicode RAW paths | Open | Use LibRaw's wide-path entry point. |
| Evaluation scale bounds | Open | Public API accepts level 31, whose scale overflows in bin2x2; GUI levels do not reach it. |
| Confirm AGPL variant | Open | Repository uses `-or-later`; original choice left `-only` versus `-or-later` unresolved. |
| End-to-end latency measurement and profiling | Open | Measure upload/draw as well as CPU work; Tracy via `profiling` is a candidate beyond frame logs. |
| Failure-safe saves and exports | Deferred | Avoid truncating an existing destination when a write fails. |
| Unsaved edits and exports on close | Deferred | Document-loss protection and completion of in-flight exports. |
| Resource memory and streamed TIFF encoding | Deferred | Revisit if retained RAWs after path changes or export buffers become costly. |
| Detect changed input files | Idea | Currently requires manual invalidation. |
| Cooperative cancellation | Deferred | Only if obsolete evaluations noticeably delay the latest edit. |
| GPU computation | Deferred | Require a measured whole-node gain and one kernel source across backends. |
| Full-detail region of interest | Deferred | Efficient 1:1 viewing without processing the whole frame. |
| Better tone mapping and color calibration | Deferred | Hue/gamut handling and calibration beyond camera matrix/as-shot WB. |
| More camera fixtures and CFA support | Deferred | Broaden validation beyond the Sony fixture; X-Trans and other non-Bayer inputs. |
| EXIF passthrough | Deferred | Carry source metadata into exports. |
| Soft-proofing | Deferred | Preview the intended output medium. |
| Display portability and measurement | Deferred | Windows/macOS paths, app-side fallback, monitor changes and colorimeter verification beyond sRGB. |
| Pop-out behavior on Windows/macOS | Deferred | Parenting above the main window and a pin control where the WM provides none. |
| Cross-window frame pacing and occlusion | Deferred | Revisit if hidden-window Fifo stalls matter in use; avoid unnecessary hidden draws. |
| Undo/redo | Deferred | Not needed for current prototype use. |
| Arbitrary deflate levels 1–9 | Blocked | Current TIFF encoder exposes only fast/balanced/best. |
