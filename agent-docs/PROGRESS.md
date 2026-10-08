# Progress

## 2026-10-08

- Removed old benchmarks, prototypes, generated reports and historical design/log notes. Kept fixtures and applicable algorithm/license credits.
- Rebuilt concrete CPU/GPU payloads, runtime meaning checks, local node declarations and request-based DAG evaluation. Intermediates retire after their last consumer; RAW normalization and metadata share an explicit source snapshot.
- Processing uses resident FP32 WGSL buffers. Frontend views share one evaluation and rendering device. Normal previews read GPU storage directly; ICC proofing and scopes have explicit host boundaries. Updated port labels, help and examples.
- Validation: 180 workspace tests passed; the final frontend run passed all 77 tests including both GPU rendering tests. Final source-cache/evaluator/example tests and six compile-fail doctests passed. All 14 library unit tests passed on lavapipe. Strict workspace Clippy, formatting and diff checks passed.
- Final real RAW demo: 1992 × 1330 at scale four, seven nodes, one image upload and one readback. Full-resolution TIFF export and the camera corpus also passed. Remaining limits are tracked in TODO.md.

- Benchmarked parent `b9b1045` against `9d43d0e` with identical release pipelines, 5.2/42.4 MP RAWs, three scales, 6/12 Rayon threads and completed GPU resident/readback modes. Full 42.4 MP recomputation: 1164/170/956 ms (Rayon/GPU resident/GPU readback). Nine serial samples per case; sampled numerical error <= 5.5e-6.
- Actual preview worker includes all scopes: at default half scale, highlights improve 641 -> 390 ms, but exposure/sigmoid regress 379 -> 396 and 359 -> 402 ms. Download profiling isolates an extra 252 ms RGB-layout copy at full size. Scripts, raw CSVs, plots and methodology are in `benchmarks/rayon-vs-wgpu/`; production code is unchanged.
