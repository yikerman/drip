# Uniform preview-worker benchmark

Status: **decided; preserve this protocol across performance changes.**

Use `fixtures/raw/sony-ilce-7rm3.arw` (7968x5320), release builds, WGPU on the
RTX 3080 and `RAYON_NUM_THREADS=12`. Run versions serially, with no concurrent
builds or device profiling. Record hardware/driver and binary/source identities.
Keep first-load/setup and warmup samples separate. Device clocks and desktop
activity are uncontrolled; record ranges and coarse GPU memory/clock samples.

The driver invokes each version's actual GUI worker with Preview, Histogram,
Waveform, Vectorscope and Export status as targets. Preserve its default graph,
parameters and all view preparation. Retain the prior presentation until the
next finishes. Time completed worker evaluation, including preparation, before
presentation cleanup. Exclude drawing, texture upload and file export.

1. Warm up levels 2, 1, 0.
2. Repeat levels 2, 1, 0 in that order five times (detail-change cases).
3. Warm up level 1, then alternate exposure +0.1/-0.1 EV for five requests.
4. Issue five identical level-1 requests explicitly. This tests worker behavior,
   not the GUI's frequency of unchanged requests.

Report medians and observed ranges for each case. Level 1 is 3984x2660 output;
Highlights remains full sensor resolution. Compare saved half-float previews
before exposure edits, including finite values and alpha. Original CPU caching
stays enabled: comparisons measure actual workflows, not equal-work kernels.
Do not infer GPU computation time from asynchronous host dispatch spans.

Keep generated measurements, source snapshots, drivers, release binaries and
LaTeX/PGFPlots outside Git, under
`../drip-benchmark-results/2026-10-09-latency/`. `current_driver.rs` is the shared
worker harness for the rewrite, Highlights port and host storage experiment;
the original driver adapts the same schedule to the former API. The source tree
used for a measurement must contain only the change being evaluated plus that
harness. Rebuild the ordinary GUI executable after snapshot builds.

Use `run_suite.py OUTPUT NAME=BINARY ...` in that artifact directory to repeat
the protocol with explicit baseline/candidate binaries. It records identities,
resource reports and one-second whole-GPU samples. Report pooled/device-wide
memory as such; it does not measure precise live allocations or transient peaks.
