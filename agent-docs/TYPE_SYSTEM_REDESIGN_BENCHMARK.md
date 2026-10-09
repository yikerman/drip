# Runtime benchmark before DAG/evaluator implementation

Date: 2026-10-08. Status: **executed specification**, responding to R24–R26 in
[the user record](TYPE_SYSTEM_REDESIGN_USER_NOTES.md). DAG/evaluator design is
paused until runtime evidence exists. This document defines the shared test
set before separate WGSL and CubeCL implementation work begins. It does not
select a runtime or change production graph code.

The experiment is complete. Results, coverage, PDF plots and LaTeX sources
were moved outside Git at the user's request:
`/home/yikerman/Developer/drip-benchmark-results/2026-10-08/`.
Reports retain their relative paths under `agent-docs/`; data and plots are
under `experiments/runtime-bench/`. The workload definitions below remain the
executed specification; optional unmeasured cases are not implied to have passed.

## Questions the experiment must answer

1. What is the cost of each real algorithm, separate from transfers, allocation,
   command preparation, submission, and completion waits?
2. How much changes when intermediates remain on the GPU, endpoints are on the
   host, or a CPU operation interrupts a GPU chain?
3. What storage remains live/reserved during a chain, fork/join, and fan-out?
   Does a runtime retain allocations after logical values die?
4. Can a node receive concrete views while the evaluator owns allocation,
   transfers, dependencies, and completion? Can GUI wgpu resources be shared?
5. What handwritten kernel, adapter, synchronization, and error-handling code
   does each approach require? Can its kernel source also execute on CPU?

Interpretations stay on the CPU. These experiments use known descriptors and
explicit placements; they do not implement the full contract checker. A small
fixed workload runner is sufficient. No persistent image-result cache and no
cross-node kernel fusion. Compilation caches and bounded allocation reuse are
separate policies and must be identified in results.

## Comparison arms and hardware

- **W:** handwritten WGSL through wgpu/Vulkan.
- **C:** CubeCL through its wgpu/Vulkan runtime, identifying whether its compiler
  emits WGSL or SPIR-V. Prefer its WGSL compiler for the most controlled pair;
  report unavailable paths and version differences rather than hiding them.
- **C-CUDA:** secondary CubeCL CUDA results if the installed toolchain supports
  them. This changes both runtime/compiler and API; keep it out of claims that
  attribute a difference solely to WGSL versus CubeCL.
- **CPU reference:** common scalar/Rayon algorithms for correctness and the
  deliberately placed CPU islands. A CubeCL CPU-runtime experiment is separate.

Pin dependencies in the standalone experiment lockfiles. Record exact versions,
backend, compiler, adapter UUID, driver, feature/limit requests, rustc, build
flags, worker count, and chosen kernel specialization/workgroup dimensions.
The production workspace currently patches wgpu; do not silently assume a
standalone CubeCL dependency uses that same revision.

Observed locally: NVIDIA RTX 3080, UUID
`GPU-8a82491f-750e-8c8a-1dba-0305ec92ed59`, 10,240 MiB VRAM, NVIDIA driver
615.71.09; native Vulkan enumeration succeeds outside the sandbox. Inside the
sandbox, NVIDIA access fails and Vulkan sees llvmpipe. Hardware benchmark
commands therefore require the tool's outside-sandbox execution path. Reject
software adapters for performance measurements. Rust is 1.99.0.

Nsight Compute/Systems and nvcc were not found on PATH in the initial probe.
That does not establish that all CUDA libraries or profilers are absent.
Agents should inspect actual capabilities and report missing metrics explicitly.
Do not install system drivers, reset the GPU, or change clocks/power settings.

## Shared algorithms and correctness

Port the mathematical algorithms, not the existing node/type/evaluator design.
The local source pin is Drip commit
`b9b1045da65376fd5812b7b00362806cde37eaf0`:

| Kernel | Source | Required behavior |
| --- | --- | --- |
| RCD Bayer demosaic | `crates/drip/src/nodes/demosaic/rcd.rs` | Full RCD stages, all four Bayer phases, measured-channel preservation, ten-pixel bilinear border, odd dimensions and tile seams. No substitution with bilinear demosaic. |
| Bayer 2×2 preview | `crates/drip/src/nodes/demosaic/bin2x2.rs` | Its specified half-resolution operation, including averaging the two greens and dropping incomplete edge cells. A separate workload, not an RCD optimization. |
| 3×3 matrix application | `crates/drip/src/nodes/camera.rs::matrix` | Explicit independent matrix input, with identical coefficients for both arms. |
| Exposure | `crates/drip/src/nodes/exposure.rs` | Multiply by the same prepared gain; no implicit clipping. |
| Full sigmoid color operation | `crates/drip/src/nodes/sigmoid/algorithm.rs` | Include positive-channel treatment, inset/outset matrices, log-logistic curve and hue/energy correction; do not benchmark just a scalar logistic function. |
| 256-bin RGB histogram | `crates/drip-gui/src/node_ui/scopes/histogram.rs::count` | Same supplied thresholds and exact integer bin counts, including edge values. Use it as a reduction workload, without moving GUI functionality into the headless library. |

RCD and sigmoid already derive from darktable commit
`61dea294bedb3ab6c7cca1a45530b1ab5c0461f3`. Preserve upstream notices and record
port changes in experiment-local `THIRD_PARTY.md` files. Root will maintain
common reference code/inputs; backend agents own their respective GPU ports.
The common reference is a comparison oracle, not proof that old Drip code is
physically correct. Check existing upstream-derived RCD/hue test vectors and
simple invariants independently of CPU/GPU agreement.

Baseline pixel arithmetic is f32; no relaxed precision or fast-math variant may
replace the baseline silently. Existing CPU sigmoid has a few f64 intermediates.
Where a GPU path needs a stable f32 reformulation, document it and test it against
the reference; do not simply narrow operations that prevent overflow. Prepared
sigmoid coefficients are identical across arms and prepared outside steady-state
timing. Any supported f16 or other approximation is a separately labeled study.

Canonical benchmark RGB storage is four f32 lanes, with RGB in the first three
and the fourth explicitly zero. Mosaic is one f32 per sample; histogram is
256 triples of u32. This is a measurement convention, not a production layout
decision. Host data uses the same padded layout in the primary comparison;
RGB3-to-RGB4 packing/unpacking is a separately measured endpoint variation.
Backends may additionally test other layouts if they report physical bytes and
packing cost. Matrix storage packing is explicit and numerically identical.

Correctness gates before timing:

- Matrix/exposure: `abs(error) <= 2e-6 + 2e-6 * abs(reference)`.
- Sigmoid: `abs(error) <= 2e-5 + 2e-5 * abs(reference)`.
- RCD: `abs(error) <= 2e-5 + 2e-5 * abs(reference)`; separately verify measured
  samples, borders, phase and tile-seam vectors. Report maximum and RMS error.
- Histogram: all integer counts equal, and each channel sums to pixel count.
- Chains: compare final values with the identical CPU operation sequence;
  derive accumulated tolerance from constituent checks and report it explicitly.

These are starting acceptance bounds fixed before backend measurements. If a
bound fails, diagnose it jointly; do not independently widen one backend's bound
to make it pass. NaN/nonfinite outputs on finite supported inputs fail. Extreme
finite values, zero, negative RGB, ties and histogram threshold neighbors belong
in correctness tests, even though ordinary bounded data drives performance runs.

After measurement, R29 clarifies that slight errors are acceptable. The observed
sparse RCD discrepancy (maximum about 0.00121456 on the photographic crop) is
accepted for this benchmark. Its original strict-check output remains recorded;
no universal replacement tolerance or algorithm change is inferred. Photographic
RCD/R1 latency was not collected during the original pass.

## Benchmark set

### Real processing scenarios

| ID | Computation | What it exercises |
| --- | --- | --- |
| R1 RAW development | Normalized Bayer → RCD → camera matrix → exposure → full sigmoid | Multi-pass stencil, scratch, point operations, real image-sized intermediates. White balance is an explicit prepared input convention, not guessed in the kernel. |
| R2 fast RAW preview | Normalized Bayer → 2×2 preview → camera matrix → exposure → full sigmoid | Small/interactive output and a different fixed demosaic node. Output dimensions are half the even input extent. |
| R3 RGB edit plus statistics | RGB → matrix → exposure → full sigmoid, with a histogram consumer on the mapped image | Heavy per-pixel math plus a reduction, two independently retainable outputs, shared image input. |

Run each kernel alone as well as its scenario. Source normalization/decoding is
outside the GPU interval. Include at least one decoded photographic fixture in
the real-scenario validation/performance report; identify its source and hash.
`fixtures/raw/pixls/nikon-d70s.nef` and `fixtures/raw/sony-ilce-7rm3.arw` are
present, not LFS pointers. Report optional decode-inclusive endpoint timing
separately so it cannot dominate or conceal runtime differences.

### Residency and CPU/GPU boundaries

| ID | Placement | Timing boundary |
| --- | --- | --- |
| P0 GPU resident | GPU → GPU chain → GPU | Data resident before start; finish when requested GPU work completes. No image download in the timed interval. |
| P1 host endpoints | Host → upload → GPU chain → download → host | Start with prepared host input; finish with accessible host output. Include staging, transfers and mandatory waits. |
| P2 preview endpoint | Host → upload → GPU chain → GPU | Include upload and compute, with no final image download. Record whether the result can be consumed on the GUI's wgpu device. |
| P3 image CPU island | Host → GPU work → download whole image → CPU exposure → upload → GPU work → host | Full-frame detour with real CPU work; preserve the same numerical pipeline and use the same common CPU function. |
| P4 small CPU island | Resident image → GPU histogram → download 3 KiB counts → CPU gain calculation → upload gain → GPU exposure | Host synchronization on a small auxiliary result while the image stays resident. Include histogram reset/reduction and the gain dependency. |
| P5 repeated boundaries | Host → alternating GPU/CPU operations → host | Two and four image detours, to expose compounded transfer/wait cost. |
| P6 round-trip control | Download/upload after every GPU node | Deliberately inefficient baseline quantifying the value of preserving residency, not a proposed evaluator policy. |

The baseline runs R1–R3 under P0/P1/P2. P3 substitutes one exposure operation
in the otherwise identical RGB chain. P4 is an explicit statistical exposure
scenario. P5/P6 use the synthetic chain below to keep workload size manageable.
Do not run an unbounded Cartesian product of every size, graph and placement.

“Normal workflow” is a hypothesis: export often needs host output, whereas GUI
preview may not. Neither endpoint profile stands in for the other.

### Synthetic workloads

| ID | Workload | Controlled variations |
| --- | --- | --- |
| S0 launch/queue floor | Tiny nontrivial write kernel whose output is validated | 1, 8, 32, 128 dependent launches; one batched submission, per-node submissions without waits, and per-node submit/wait. Label each separately. |
| S1 bandwidth chain | Out-of-place RGB affine kernel `y = 0.999*x + 0.001` | 1, 8, 32 nodes; P0/P1/P3/P5/P6. Every dispatch executes; no algebraic collapse or cross-node fusion. |
| S2 arithmetic intensity | Per-element recurrent multiply/add, result stored | 16, 64, 256 dependent iterations per element; same input/output bytes and explicit operation counts. Check generated work is not optimized away. |
| S3 fork/join | Shared image → 2 or 4 affine branches → pairwise blend join | Serial completion, queueing ready branches, and overlapping queues/streams only if actually supported. Record timeline overlap rather than assuming it. |
| S4 multi-output/fan-out | Image plus histogram; reuse image in 1 or 4 consumers | One producer evaluation, separate output lifetimes, one upload per reusable CPU matrix/gain in the controlled runner. Do not replace independent outputs with a packed blob. |
| S5 memory/lifetime | Long chain and wide fork with bounded allocations | Fresh allocations versus bounded pool reuse; drop at last dependency versus deliberately retain all. Sweep 256 MiB, 1 GiB, 4 GiB application budgets, stopping before allocation failure. |
| S6 repeated edits | Re-evaluate one warm fixed graph while changing exposure/sigmoid parameters | No image-result cache; record pipeline recompilation/specialization count, allocation reuse, latency, and whether warm reserved memory stabilizes over 100 edits. |

S5 does not fill the entire GPU intentionally. Count inputs, outputs, scratch,
transfer staging, query buffers and reserved pools. Use budget admission and
report skipped sizes; the 4 GiB application budget is not a 4 GiB physical GPU
simulation. Other applications already use some VRAM on this machine.

### Sizes and inputs

- Correctness: tiny/empty where supported, 19×17, 277×269 with all Bayer phases,
  odd extents and dispatch/tile boundaries. Empty work must not dispatch invalid
  workgroups or masquerade as a measured nonempty workload.
- Interactive: 256×256 and 1920×1080.
- Full resolution: 6000×4000 (24 MP).
- Pressure: 8192×5464 (~45 MP), only where explicit live-byte estimates fit.
- Real fixture: native size and a documented crop; no silent resizing of Bayer
  data that changes its phase or sampling convention.
- Synthetic inputs: deterministic common generator, natural-image-like ramps
  plus textured/noisy patterns, HDR RGB and negative values, flat fields, saturated
  highlights. Histogram runs include both distributed and highly concentrated
  values so atomic contention is visible.

## Measurement protocol

Both agents may build and inspect code concurrently. **Only one GPU benchmark
process at a time**, including correctness/tuning runs. Root coordinates a GPU
lease; agents request it before running GPU commands and return it afterward.
Do not overlap heavy compilation with final CPU-overhead measurements. Final
comparison uses alternating backend blocks on the same warmed device.

1. Record context/device initialization and first pipeline/JIT compilation
   separately. A fresh process is not proof of an empty driver shader cache;
   label process-cold and the observed cache state. Do not erase user caches.
2. Warm each case at least five times; collect at least twenty complete timings
   per representative case in three blocks for final comparisons. Report median,
   p95, dispersion, sample count, and raw samples. Initial exploratory smoke
   runs may be shorter but must be labeled preliminary.
3. Preallocate/reuse storage for the warm compute-only test. Separately measure
   an end-to-end allocation-inclusive request. Keep input preparation, validation,
   printing and unrelated file I/O outside the defined interval.
4. Host wall time ends at actual completion or accessible host output, as the
   case requires. Enqueue time alone is never called execution time. GPU timing
   uses timestamp queries/events where supported, resolving them after batches.
5. Separately measure command preparation/submission, explicit host wait time,
   CPU island time, transferred bytes and transfer completion. These may overlap;
   do not claim that subtracting GPU time from wall time isolates pure overhead.
6. Collect instrumented profiles separately from primary latency runs and report
   their overhead. For utilization sampling, repeat a steady workload for at least
   two seconds so short dispatches are not inferred from isolated samples.

| Metric | Measurement and limits |
| --- | --- |
| Latency and throughput | Completed host wall time, GPU timestamp spans, MP/s; distinguish full-request and device-only intervals. |
| Memory | Exact logical/live/allocated bytes from runner bookkeeping; runtime reserved bytes if available; process RSS and sampled GPU-memory delta separately. Driver/device-wide deltas are not exact per-node peaks. |
| Transfers | H2D/D2H bytes including packing/staging; isolated completed copy timings plus scenario-inclusive cost. |
| CPU cost | Wall/CPU time for preparation, submission, waits and CPU work; record worker count. |
| GPU utilization | Sample NVIDIA telemetry with timestamps during a sustained case. It reports activity over sampling windows, not occupancy or achieved arithmetic efficiency. |
| Compute/memory efficiency | Effective logical bandwidth, known synthetic FLOP/s, plus actual profiler SM/DRAM/occupancy/register/spill counters if accessible. Label estimates and unavailable counters. Do not assign a fake FLOP count to transcendental sigmoid operations. |
| Scheduling | Dispatch/submission/wait counts, ready-branch timeline, observed overlap, kernel stage count and completion boundaries. |
| Build/runtime overhead | Clean and incremental build timing, first kernel compilation, repeated parameter/shape changes, number of specializations. |

Native wgpu timestamp queries have explicit feature requirements and return
device time units requiring the queue's timestamp period [1]. CubeCL offers
runtime tracing facilities [2]; agent reports must distinguish these logs from
hardware counters. NVIDIA utilization is a sampled activity measure [3]; Nsight
Compute's profiler metrics and replay effects are a separate profiling layer [4].

Following R28, optimization is deliberately limited. Reasonable optimization
means contiguous access, sensible workgroup/vector sizes,
explicit edge handling, avoiding redundant host transfers/allocations, and shared
or partial histogram accumulation rather than one globally contended atomic per
sample where practical. A correct global staged RCD implementation is sufficient;
shared-memory tiling and exhaustive workgroup searches are not required. Each agent
records any tested candidates and its selected variant. A simple correct baseline
must remain reproducible. Tuning may specialize a fixed algorithm, never replace
it with a cheaper one. Fusion inside one algorithm is allowed and documented;
fusion across benchmark nodes is excluded.

## Observations during implementation

Common CPU oracles pass pinned upstream-derived RCD and sigmoid-hue vectors,
flat fields, histogram threshold neighbors and extreme finite sigmoid checks.
Nikon D70s fixtures have been decoded once into native 3038×2014 and origin
1024×768 crops; data hashes and preprocessing are recorded separately.

Initial WGSL photographic RCD validation exposed four channels beyond the
declared tolerance despite passing synthetic phases and seams. At (107,714),
approximately complementary direction estimates lie near a discontinuous
confidence-selection tie: small float differences select substantially different
directions. This is under investigation and is not an approved tolerance change.
Both agents must retain and report the baseline result. Extreme finite sigmoid
inputs also require stable GPU reformulation; these remain correctness work,
not optional speed tuning. Backend reports contain the eventual resolutions.

## Deliverables and division of work

Experiments live under `experiments/runtime-bench/`, with standalone manifests
so production dependencies and `Cargo.lock` remain unchanged.

- Root owns `common/`, `scripts/`, shared inputs, this specification, integration,
  comparison and GPU scheduling.
- WGSL agent owns `wgsl/`, its results and
  `agent-docs/TYPE_SYSTEM_REDESIGN_BENCHMARK_WGSL.md`.
- CubeCL agent owns `cubecl/`, its results and
  `agent-docs/TYPE_SYSTEM_REDESIGN_BENCHMARK_CUBECL.md`.
- Each agent supplies runnable CLI commands, pinned sources/licenses, correctness
  output, raw timing JSON/CSV, memory accounting and profiler/telemetry provenance.
  Missing cases/metrics must be listed, not represented as zero or passed.
- Neither agent edits production graph/evaluation code or the original design,
  progress or TODO logs. Proposals stay separate from user requirements.
- Following R27, root will generate LaTeX/PGFPlots figures and rendered PDFs
  from retained raw samples. Plot timing boundaries/units and variability;
  distinguish unavailable metrics and correctness failures from measured zeros.

The shared implementation should provide deterministic data and CPU references
for matrix/exposure/sigmoid/RCD/bin2x2/histogram. Agents may begin runtime setup
and shader ports while root completes that crate, but must use common inputs and
oracles for final comparisons. Both ports must use the same prepared sigmoid
coefficients and histogram thresholds.

Reports assess concrete typed views, host descriptors, externally owned output
allocations, device/queue sharing, buffer lifetime visibility, cancellation and
in-flight ownership, CPU kernel reuse, diagnostics, portability, dependency/API
friction, handwritten code size excluding generated/vendor code, and a small
change exercise (add a parameter and an auxiliary output). Do not implement a
general DAG just to demonstrate these points. CPU/GPU one-source feasibility is
a requirement to investigate, not something WGSL or CubeCL earns by assertion.

Initial execution order: common references → individual kernels and correctness
→ R1–R3/P0–P2 → CPU islands/synthetic scheduling → memory/edits → reviewed paired
measurements. If a runtime blocks a stage, record the exact limitation and keep
the successful comparable subset. No performance winner is declared from an
incomplete algorithm, a failed correctness gate, or unlike timing boundaries.

## References

[1] gfx-rs contributors, “QueryType” and “ComputePassTimestampWrites,” *wgpu
30.0.1 API documentation*. [Online]. Available:
[query timing](https://docs.rs/wgpu/30.0.1/wgpu/enum.QueryType.html) and
[pass timestamps](https://docs.rs/wgpu/30.0.1/wgpu/struct.ComputePassTimestampWrites.html).

[2] Tracel contributors, “Performance Hacking,” *CubeCL*, accessed Oct. 8, 2026.
[Online]. Available: [runtime tracing](https://github.com/tracel-ai/cubecl/blob/main/PERFORMANCE.md).
Implementation reports must pin their actual CubeCL version/revision.

[3] NVIDIA, “NVIDIA System Management Interface,” accessed Oct. 8, 2026.
[Online]. Available: [nvidia-smi documentation](https://docs.nvidia.com/deploy/nvidia-smi/index.html).

[4] NVIDIA, “Profiling Guide,” *Nsight Compute*, accessed Oct. 8, 2026.
[Online]. Available: [profiling metrics and methodology](https://docs.nvidia.com/nsight-compute/ProfilingGuide/index.html).
