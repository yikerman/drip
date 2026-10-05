# TODO

Ideas not yet implemented. Design references point to `docs/DESIGN.md`.

| Idea | Source | Status | Notes |
|------|--------|--------|-------|
| CLI batch implementation | handoff | deferred | scaffolded; template binding per F4 |
| GPU computation | user | postponed | Rayon replaces CubeCL; reconsider only with a measured need and one kernel source (E9, E13, E15) |
| End-to-end UI latency | measurement | open | Rayon level-0 evaluation is about 162 ms, excluding texture preparation/upload/drawing; evaluation still blocks the UI (E12, DESIGN 3.3.3) |
| Manual global preview level | user | planned; not implemented | selector and persistence; remove automatic adaptation; export stays at level 0 (E16, DESIGN 3.3.4) |
| Simplify RAW retention | user | planned; not implemented | remove eager normalized pyramid; proposed decoded-data retention for active project and export, with explicit reload (E17, DESIGN 3.3.4) |
| Background evaluation | user | proposed; not implemented | persistent preview worker, latest pending request, stale-result rejection, ordered export/reload; move texture packing and buffer retirement off UI (E12, DESIGN 3.3.4) |
| Full-res region of interest for 1:1 viewing | handoff | deferred | ROI in `EvalContext` (E4) |
| EXIF passthrough | handoff | deferred | `RawMetadata` travels the graph (P3) |
| Soft-proofing | handoff | deferred | extra transform in the display path |
| Windows/macOS display color paths | handoff | deferred | |
| X-Trans and other non-Bayer CFAs | design | deferred | general CFA representation (P5) |
| Detect input files changing on disk | design | idea | beyond manual reload (E2) |
| Static LibRaw build with only the features we need | user | planned | vendored source, built with `cc`, unused decoders and demosaics and the DNG SDK/RawSpeed glue disabled; replaces linking system `libraw_r` (L3) |
| App-side display transform | design | deferred | for compositors without color management (D3) |
| Arbitrary deflate levels 1-9 | user | blocked | `tiff` 0.11 only offers fast/balanced/best (C5) |
| Unicode raw paths on Windows | M2 | open | LibRaw takes narrow paths; needs `libraw_open_wfile` there |
| Relative paths against the project file | design | open | needs the project's location; with the CLI (F2) |
| Budget and eviction for loaded resources | design | deferred | reconsider after the planned active-project retention policy; current code still retains session RAW pyramids (E11, E17) |
| Test raws from more cameras | user | postponed | e.g. CC0 samples from raw.pixls.us; one Sony fixture for now |
| Better tone mapper | user | postponed | hue-preserving and gamut-aware, beyond the per-channel sigmoid (C4) |
| Color calibration | user | postponed | beyond the camera's built-in matrix and as-shot white balance (C3) |
| Undo/redo in the GUI | user | postponed | G4 |
| Colorimeter check of the wide-gamut display path | user | postponed | screenshots only verify up to sRGB (D5) |
| Enlarge a preview in its own window | user | postponed | egui-free viewer window sharing the GPU device (G7) |
