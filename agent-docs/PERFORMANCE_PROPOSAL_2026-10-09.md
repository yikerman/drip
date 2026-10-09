# Preview performance proposal

Status: **step 1 implemented and validated; steps 2–4 remain tentative.**

The user requested a subagent port of opposed highlights first, and an
explanation of steps 2–4. Implementation and validation are recorded in
`OPPOSED_CUBECL_PORT.md`. Measurements below describe the pre-port baseline.

## Requirements

- Preserve node functions, port contracts, the shared global context and GUI
  interactions. Keep processing kernels with their nodes and presentation under
  `drip-gui/src/node_ui/`.
- Keep one production implementation of each kernel across compute backends.
- Tracing must not add synchronization. Dedicated device-timestamp experiments
  remain separate from normal latency measurements.
- Do not restore an unbounded graph cache or change preview algorithms to make
  the benchmark faster.
- Benchmark outputs, plots and instrumented snapshots remain outside Git.

The evidence is in the [benchmark report](../../drip-benchmark-results/2026-10-09-latency/results/report.md)
and [LaTeX plots](../../drip-benchmark-results/2026-10-09-latency/results/comparison.pdf).
The original CPU baseline is commit `b9b1045`; the current baseline is the
working tree before this investigation. Both run the real GUI worker on the
Sony A7R III fixture in release mode.

## Findings that determine the proposal

The main regression is interactive recomputation combined with expensive host
materialization. Exposure edits used to reuse sensor processing and demosaic;
they now repeat the whole dependency graph. Full-detail recomputation is faster
in the rewrite, so this is not a general failure of GPU processing.

The current image route is:

```
CPU RAW -> GPU white balance -> CPU highlights -> GPU RCD/color -> CPU views
```

White balance creates an early device excursion immediately before CPU
highlights. Its cost includes a full-resolution upload, download and extra host
copies. Highlight computation itself accounts for only part of the reported
highlight-node span. RCD's host span includes another upload. The preview's
evaluation span includes waiting for preceding GPU work and unpacking the result.

CPU preview packing and scope computation also cost substantial time, but their
algorithms predate the rewrite. Merely changing the kernel language would leave
these costs and the ownership/placement decisions intact.

The no-intermediate-cache requirement deliberately gives up the original
exposure-edit reuse. GPU acceleration alone does not imply that every edit will
be faster. Any reuse proposal must acknowledge that tradeoff explicitly.

## 1. Keep the sensor processing chain on the device

**Proposed first processing change:** port opposed highlight reconstruction to
CubeCL and change its image ports to device storage. Keep the existing
Highlights node, parameters, clipping-level input and interpretation contract.
Its mask, dilation, chrominance reduction and reconstruction passes are private
kernels in `node/highlights/`; they are not new DAG nodes.

This leaves one large image upload before white balance and one image download
after color processing. It also replaces the full-resolution CPU highlight work.
The small clipping-level calculation can remain a CPU node; moving a few
coefficients is not the expensive boundary here.

Use portable reductions with explicit accumulation order/precision choices;
do not assume floating-point atomics or introduce cross-workgroup races.
The global chrominance reduction and nearly clipped samples need comparison
against the existing opposed reference tests. Port the algorithm before tuning
its workgroup layout. CPU fallback uses the same CubeCL kernels.

An alternative is keeping white balance on the CPU beside highlights. That is
a useful experiment and a smaller placement change, but retaining the old
handwritten CPU white balance beside a CubeCL implementation would violate the
single-source requirement. I would not silently introduce that duplication.

**Validation:** compare output and CPU fallback against the existing fixtures;
repeat the complete worker benchmark; confirm the early large download and the
following large upload disappear. Measure both live and pooled device memory,
including full-detail RCD, before accepting the port.

## 2. Make host-buffer ownership explicit at transfers

The current `Vec`-based bridge clones Bayer input before upload and copies a
mapped download into a new vector. RGB additionally changes from device RGBA
to packed host RGB. These are real memory operations outside the kernel math.

**Proposed narrow design investigation:** a concrete typed host buffer that
owns its storage and exposes checked slices, with uploads retaining the owner
until completion. Keep the ownership/transport machinery shared; payloads still
declare their concrete layouts and interpretations. Node algorithms should
continue to consume immutable input slices and write freshly allocated outputs.

CubeCL's pinned version supports owned `Bytes` and reference-counted shared
views. Investigate using that facility before writing a custom allocator or
unsafe allocation controller. Avoid a new collection of residency traits or
node-specific transfer policies.

Do **not** assume retaining mapped readback memory is automatically faster.
Multi-pass CPU algorithms may read that storage less efficiently than ordinary
RAM, and retaining mappings can prevent staging-buffer reuse. Compare direct
views with one deliberate copy, including peak memory and lifetime behavior.
Similarly, do not force all CPU payloads into RGBA just to simplify one transfer.

**Validation:** count eliminated copies, preserve fan-out and last-use release,
test errors and outstanding uploads, and benchmark total consumer time rather
than only the transfer call. No implicit waits may be introduced by tracing or
buffer accessors.

## 3. Prepare GUI results near their device input

**Proposed follow-up:** let the existing GUI nodes produce presentation-sized
results from device RGB: packed preview texels and compact scope counts. Keep
the operations in `node_ui`; do not add canvas nodes or move GUI algorithms
into the headless crate. Preserve full-image histogram/scope semantics rather
than substituting a thumbnail approximation.

Start with preview packing, which is currently a scalar CPU conversion of every
channel. Preserve half-float rounding, negative values, HDR values and alpha.
Scopes need their current binning, clipping and reduction behavior checked too.
CPU and GPU execution should share the new kernels.

This can initially return compact host results to the existing renderer.
Cross-library GPU texture sharing is a separate decision: it is not necessary
to remove the current full-float readback plus CPU packing. Proofing and export
may continue to need host RGB; transport should materialize it only for actual
consumers and share it within that evaluation.

**Validation:** unchanged drawing and inspector behavior; identical preparation
contracts; histogram counts and existing waveform/vectorscope tests; measure
texture-upload time separately from worker time. Do not optimize the default
preview by breaking softproofing or batch export.

## 4. Treat reuse as an explicit policy decision

**Proposed independently:** avoid a redundant identical worker request while
its completed presentations are still available. Its identity must include
computation changes, resource invalidation, global context and targets; layout
changes alone should not invalidate processing. This retains no additional
intermediate image. Pending-request coalescing is already present and does not
by itself provide this behavior. First audit callers to establish how often
identical requests actually occur; the benchmark deliberately exercises one.

**Open, not part of the first implementation:** bounded intermediate reuse for
exposure/sigmoid edits. A single useful checkpoint before the edited suffix can
avoid sensor processing without caching every node. Any future policy needs a
byte budget, correct upstream/context/resource invalidation, and normal
recomputation when the value does not fit. Device storage, aliases and staging
lifetimes must be counted rather than treating every handle as a separate image.

I would first measure steps 1–3 with intermediate caching still disabled. If
interactive latency remains unacceptable, discuss a bounded checkpoint against
the original no-cache constraint. Do not promise the old cached edit latency
while requiring every edit to recompute the entire image.

## Acceptance and order

Implement each accepted step separately and rerun the same original/current
worker cases: scale changes, exposure edits and explicit unchanged requests.
Keep first-load cost separate; report ranges and output differences. A change
must improve completed work, not merely shift waiting into another node.

Start with the device-resident highlight path and the host upload-copy design;
then address GUI preparation. Keep fusion, asynchronous branch scheduling,
cross-library texture interop and automatic backend selection out of this work.
The measurements do not justify those complications yet.
