# Progress

## 2026-10-08

- Removed old benchmarks, prototypes, generated reports and historical design/log notes. Kept fixtures and applicable algorithm/license credits.
- Rebuilt concrete CPU/GPU payloads, runtime meaning checks, local node declarations and request-based DAG evaluation. Intermediates retire after their last consumer; RAW normalization and metadata share an explicit source snapshot.
- Processing uses resident FP32 WGSL buffers. Frontend views share one evaluation and rendering device. Normal previews read GPU storage directly; ICC proofing and scopes have explicit host boundaries. Updated port labels, help and examples.
- Validation: 180 workspace tests passed; the final frontend run passed all 77 tests including both GPU rendering tests. Final source-cache/evaluator/example tests and six compile-fail doctests passed. All 14 library unit tests passed on lavapipe. Strict workspace Clippy, formatting and diff checks passed.
- Final real RAW demo: 1992 × 1330 at scale four, seven nodes, one image upload and one readback. Full-resolution TIFF export and the camera corpus also passed. Remaining limits are tracked in TODO.md.
