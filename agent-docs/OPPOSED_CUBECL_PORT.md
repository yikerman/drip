# Opposed CubeCL port

Status: **decided; step 1 authorized and implemented.** Host buffer ownership,
GUI preparation and reuse (proposal steps 2–4) remain unimplemented proposals.

## Requirements

Keep the single Highlights DAG node, threshold parameter, Rustdoc, phase and
interpretation contracts, and independent four-site BayerLevels input. Large
Bayer input/output ports use Device storage. The production algorithm must be
one portable CubeCL implementation for WGPU and optional CPU computation.
No intermediate image readback, added synchronization, float atomics, GUI or
macro changes, graph caching, canvas nodes, or backend fallback selection.

## Decisions

- **Decided:** retain kernels in `node/highlights/opposed.rs`. The previous
  Rust/Rayon algorithm is compiled only under `cfg(test)` in `reference.rs`.
  It provides an independent numerical check beside pinned upstream C vectors.
- **Decided:** keep clipping levels on the CPU and pass scaled four-site levels
  as scalar kernel arguments. This preserves unequal green-site values and
  avoids uploading a coefficient allocation.
- **Decided:** build one u32 bit mask per partial 3x3 cell and dilate it with
  upstream's 7x7 footprint excluding corners, clipped at sensor boundaries.
  The absent-clipping case takes the same dispatch route and preserves samples.
- **Decided:** reduce chrominance in fixed blocks of 256 consecutive samples,
  then reduce 256 partials per invocation until one three-channel result remains.
  Each invocation writes disjoint storage; separate dispatches enforce global
  dependencies without atomics or cross-workgroup communication. Scratch counts
  are u32, exact throughout every valid image extent; sums are f32 for portable
  WGPU support. The prior reference uses f64 row accumulation. This changes
  grouping/precision, not sample selection or the strict greater-than-100 cutoff.
- **Decided:** use nonnegative f32 powers with exponent 1/3 for cube roots.
  Reconstruction retains the input maximum and leaves unclipped samples intact.
  No dispatch reads large intermediates back to the host.
- **Decided:** use private typed u32 scratch for masks/counts and the existing
  f32 scratch allocator for sums. Scratch lifetime follows queued dispatches;
  the compute server owns outstanding buffer uses after local handles drop.

## Validation

Module-local tests run the same kernels under CPU and WGPU, compare all four CFA
phases against the previous reference and pinned upstream CSV, exercise odd
extents, partial cells, 1-pixel dimensions, absent clipping, unequal green levels,
negative/signed-zero values, floats immediately around clipping thresholds and
multiple global reduction passes. A constructed fixture verifies exact 100/101
eligible-sample behavior. A varying 277x269 fixture reconstructs more than 100
samples in every CFA phase, checking nontrivial learned chrominance through two
global reduction passes. A Device producer -> Highlights -> Device consumer
integration test passes on CPU and WGPU and verifies one image upload and one
final download. The actual GUI worker was benchmarked against the original CPU
implementation and pre-port rewrite, including full-detail processing.

Benchmark measurements, image comparisons and the source snapshot remain outside
Git: [report](../../drip-benchmark-results/2026-10-09-latency/results-opposed/report.md),
[LaTeX plots](../../drip-benchmark-results/2026-10-09-latency/results-opposed/comparison.pdf).
Device memory sampling includes desktop usage and allocator pools; it does not
isolate live allocations or guarantee observation of transient peaks. The port
improves latency while increasing sampled peak device memory. No intermediate
cache or host-buffer ownership change was included. The release GUI build and
Clippy for the library/tests with CPU support pass.

Focused validation passed on 2026-10-09:

```sh
CC=/usr/bin/cc CXX=/usr/bin/c++ CARGO_TARGET_DIR=target/cubecl-rewrite \
  cargo test -p drip --features cpu --offline --lib node::highlights -- --nocapture
CC=/usr/bin/cc CXX=/usr/bin/c++ CARGO_TARGET_DIR=target/cubecl-rewrite \
  cargo test -p drip --features cpu --offline --lib \
  node::highlights::tests::wgpu_opposed -- --ignored --nocapture
```

The first command passed four tests (three independent-reference checks and the
CubeCL CPU suite), with the hardware WGPU test ignored. The explicit WGPU run
passed. Maximum absolute deviation from the f64 reference across tested samples
was 9.54e-7 on CPU and 1.55e-6 on WGPU. Both passed the pinned upstream vectors
within their existing 2e-5 bound. Unclipped samples retained their bit patterns;
the 100/101 sample fixture produced its analytic 2.0/1.0 results on both backends.
