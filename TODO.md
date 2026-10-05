# TODO

Ideas not yet implemented. Design references point to `docs/DESIGN.md`.

| Idea | Source | Status | Notes |
|------|--------|--------|-------|
| CLI batch implementation | handoff | deferred | scaffolded; template binding per F4 |
| One-source CPU/GPU kernel experiment | user | proposed | CubeCL first candidate; verify platform builds, single-worker/parallel CPU, GPU, numerical behavior, JIT latency and wgpu buffer sharing; no duplicate kernels (E9, E10) |
| GPU processing | handoff | deferred | depends on the kernel experiment; keep intermediate buffers on device through preview drawing |
| Background evaluation | user | proposed | persistent worker, retain last preview, coalesce requests and discard obsolete results; cancellation between nodes/chunks (E12, revises G2 if agreed) |
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
| Budget and eviction for loaded resources | design | deferred | user accepts session-long RAW pyramids without eviction for the prototype; about 226 MB per 42 MP file (E11) |
| Test raws from more cameras | user | postponed | e.g. CC0 samples from raw.pixls.us; one Sony fixture for now |
| Better tone mapper | user | postponed | hue-preserving and gamut-aware, beyond the per-channel sigmoid (C4) |
| Color calibration | user | postponed | beyond the camera's built-in matrix and as-shot white balance (C3) |
| Undo/redo in the GUI | user | postponed | G4 |
| Colorimeter check of the wide-gamut display path | user | postponed | screenshots only verify up to sRGB (D5) |
| Enlarge a preview in its own window | user | postponed | egui-free viewer window sharing the GPU device (G7) |
