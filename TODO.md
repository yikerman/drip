# TODO

Ideas not yet implemented. Design references point to `docs/DESIGN.md`.

| Idea | Source | Status | Notes |
|------|--------|--------|-------|
| CLI batch implementation | handoff | deferred | scaffolded; template binding per F4 |
| GPU computation | user | postponed | Rayon replaces CubeCL; reconsider only with a measured need and one kernel source (E9, E13, E15) |
| End-to-end UI latency | measurement | open | image evaluation, texture preparation and CPU image retirement now run off-thread; measure remaining driver upload/draw latency at full detail (DESIGN 3.3.6) |
| Profiler integration | user | open | Tracy through the `profiling` crate, which egui, epaint, egui-wgpu and wgpu already instrument; the frame log covers coarse timings |
| Cooperative evaluation cancellation | design | deferred | running evaluations finish before the latest pending request; consider a between-node check only if stale-work latency warrants it (DESIGN 3.3.6) |
| Full-res region of interest for 1:1 viewing | handoff | deferred | ROI in `EvalContext` (E4) |
| EXIF passthrough | handoff | deferred | `RawMetadata` travels the graph (P3) |
| Soft-proofing | handoff | deferred | extra transform in the display path |
| Windows/macOS display color paths | handoff | deferred | |
| X-Trans and other non-Bayer CFAs | design | deferred | general CFA representation (P5) |
| Detect input files changing on disk | design | idea | beyond manual cache invalidation (E2, G10) |
| Static LibRaw build with only the features we need | user | planned | vendored source, built with `cc`, unused decoders and demosaics and the DNG SDK/RawSpeed glue disabled; replaces linking system `libraw_r` (L3) |
| App-side display transform | design | deferred | for compositors without color management (D3) |
| Arbitrary deflate levels 1-9 | user | blocked | `tiff` 0.11 only offers fast/balanced/best (C5) |
| Unicode raw paths on Windows | M2 | open | LibRaw takes narrow paths; needs `libraw_open_wfile` there |
| Relative paths against the project file | design | open | needs the project's location; with the CLI (F2) |
| Budget and eviction for loaded resources | design | deferred | decoded RAW resources last until project replacement or invalidation; path changes within a project retain earlier reads (E17, E19) |
| Test raws from more cameras | user | postponed | e.g. CC0 samples from raw.pixls.us; one Sony fixture for now |
| Better tone mapper | user | postponed | hue-preserving and gamut-aware, beyond the per-channel sigmoid (C4) |
| Color calibration | user | postponed | beyond the camera's built-in matrix and as-shot white balance (C3) |
| Undo/redo in the GUI | user | postponed | G4 |
| Colorimeter check of the wide-gamut display path | user | postponed | screenshots only verify up to sRGB (D5) |
| Enlarge a preview in its own window | user | postponed | egui-free viewer window sharing the GPU device (G7) |
