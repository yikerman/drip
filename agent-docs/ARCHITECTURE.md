# Architecture

The design as the code implements it on 2026-10-13. [DESIGN](DESIGN.md)
records why, [TODO](TODO.md) lists unfinished work and
[PROGRESS](PROGRESS.md) holds the evidence.

## Reading order

1. `AGENTS.md`: project rules.
2. This file: values, shapes, evaluation and how to add a node.
3. The [drip](../crates/drip/README.md) and
   [drip-gui](../crates/drip-gui/README.md) READMEs: files, registries,
   backends and worked examples.
4. [DESIGN](DESIGN.md): the decisions behind both.

[COMPUTE_MODEL](COMPUTE_MODEL.md), [FIELD](FIELD.md) and
[TYPE_SYSTEM](TYPE_SYSTEM.md) are research references.

## Value model

A node computes context × parameters × inputs → outputs. Each port names a
payload, a type implementing `Payload` (`payload.rs`) with a host value type
`Host`, a description type `Desc` and `byte_len`, the bytes one value
occupies. A description holds what a contract reads without touching
samples: extent, CFA phase, colour coordinates. Payloads that kernels read
also implement `DeviceLayout`, which fixes their byte layout on the device.

The value on an edge is an `Image` (`image.rs`): a type-erased description
and either a raster or a deferred computation. Clones share one value.

A `Raster<P>` holds materialized samples. It is immutable, shared by
clones, and holds the description and at least one of a host and a device
representation. The device representation is a `DeviceBuffer<P>` on the
runtime that made it. The missing one is made once, on first request:
`host()` downloads, waiting for the kernels that write the buffer, and
checks the result against the description. `device(ctx)` uploads on the
context's runtime. The runtime counts each transfer. Only `DeviceLayout`
payloads have `device`, so a device read of host-only `CaptureMetadata` does
not compile. `share_as` reads the same storage as another payload under a
description of equal byte length. Nothing writes a published raster, so
fan-out and forwarded outputs share storage while nodes stay pure.

A deferred image holds an input image and a `Plan` (`plan.rs`): one stage,
then the map bodies to apply and their constants. The stage resamples,
through per-channel projective maps from the image's plane to the input's
plane with their filter, or runs a stencil over the input's lattice
([Stencils](#stencils)). The input is a raster, or a deferred image where
the plan cuts (see [Segments](#segments)). `Image::materialize` materializes
the input, runs the plan once as one fused kernel into a device raster and
drops the plan and its input, so a segment never stores its intermediate
images. Later calls and clones share the result. `Image::raster`
materializes and returns the samples, for frontends. `Image::crop(window,
ctx)` materializes only a window of the image's lattice: a deferred image
runs its plan on that window and on the part of its input the window reads,
and a raster holding more copies the window in one launch. The result is a
raster whose description states its window (see [Data model](#data-model)).
`Image::fold` and `Image::count` reduce the samples an image holds to a few
host values without storing them: a deferred image's plan runs per pixel
inside the reduction's pass ([Reductions](#reductions)).

A payload whose samples sit on a plane, today `Rgb`, sets `Payload::PLANE`
to `Plane::of::<P>()`. The `Plane` holds the payload's lattice, byte length,
box reduction and crop as functions, so evaluation reduces and crops images
it knows only as `Image`. Other payloads stay whole and at the detail their
producer chose.

The `Store` (`store.rs`) is the evaluator's memo of materialized values. A
value's key is a SHA-256 over its producer's kind ID and `Kind::version`,
the node's `Identity` (`definition.rs`), the requested scale when the kind's
shape is scaled ([Shapes](#shapes)), and the keys of its inputs in port
order. A computed node's identity is its parameter JSON. An image reduced to
the requested scale and the part of an image a region covers have keys of
their own, derived from the image's (`Key::reduced`, `Key::region`), so two
crops are two entries and the whole image a third. A foreign node's identity
is its parameters and the stamps (path, length, modification time) of the
input files they name, taken at every evaluation, without the scale. Bound
in-memory data (`node::source`) has a per-instance random identity, and keys
derived from it never reach the disk. Equal keys mean equal samples, so
equal nodes in two graphs share an entry. Entries are rasters, never
replaced, each with its producer's `Durability`: High for foreign nodes and
bound data, otherwise the most durable of the kind's tags (`registry::TAGS`
states one per tag), Low without tags. The `RuntimeContext` sets a `Budget`
per side, by default half the device memory the runtime reports or 4 GiB,
and 8 GiB on the host. Over a budget the store drops representations from
the least durable entry, the least recently used first among equals.
Dropping a device representation keeps the host one, and an entry with
neither leaves. A deferred image enters once a consumer has materialized it,
so the store never holds a plan's input. Dropping the `Evaluator` drops its
store.

The store also runs foreign nodes as jobs, one per node key, each on a
thread of its own, and keeps a failed job so its error repeats for its key
without running again. With `Evaluator::with_disk(Disk)` it has a disk tier
for the outputs of `foreign = disk` nodes: one file per output in
`Disk::directory`, named by the key in hexadecimal and holding a format line,
the payload name and the payload's `Persist` stream (`payload.rs`). A job
writes its files before it ends, under a temporary name renamed into place.
A demanded value missing in memory is read from disk into a host raster and
kept as a High entry. A file that fails to read is deleted and counts as a
miss. Over `Disk::budget` (16 GiB by default) the least recently read or
written files go first. `Disk::platform()` is `drip/store` in the platform
cache directory, `$XDG_CACHE_HOME` or `~/.cache` on Linux. `Evaluator::new`
has no disk tier.

## Shapes

`Kind::shape` (`shape.rs`) declares how evaluation runs a kind. `#[node]`
derives it from the declaration: `foreign = memory` or `foreign = disk` with
`Publish` ports makes a foreign node, `map = body`, `warp`, `resample`,
`reduce = BODY`, `bins = BODY` or `stencil = BODY` with two `Point` ports
make a map, warp, resample, reduce, bins or stencil node, `meta` with
`Inlet` and `Outlet` ports makes a meta node, and any other function is a
kernel, `Shape::Kernel { scaled }`. `Source` is the shape of bound in-memory
data, `node::source`, which hands each run its raster.

`Shape::scaled` is true for a kernel declared `#[node(scaled)]`, a warp and
a resample. Their contracts and kernels receive the requested scale and
produce their output at it: RCD and Bayer 2×2 demosaic reduce each CFA phase
plane before demosaicing, and Perspective Transform and Resample store
`div_ceil(full, scale)` samples. Every other node receives scale 1 and
processes the samples it receives ([Evaluation](#evaluation)). A reduction's
output has no lattice, so its shape is not scaled. It still runs at the
requested scale when it reads an image on a plane, whose key carries the
scale (`Key::reduced`).

| Shape | Function | Inputs | Outputs | Evaluation |
|---|---|---|---|---|
| `Kernel` | `fn(&KernelContext, &Params, ports...) -> Result<()>`, with a contract when it has outputs | `Read<'_, P, R>`, or `Option<Read<..>>` when optional | `Write<'_, P, R>`, written on host or device, or `Forward<'_, P, R>`, set from a raster the node holds | the contract, then the function on whole materialized inputs, whenever a demanded output is not stored |
| `Foreign(Cache)` | `#[node(foreign = memory \| disk)] fn(&Params, Read..., Publish...) -> Result<()>`, without context or contract | `Read<'_, P, R>`, materialized, with host samples downloaded before the call | `Publish<'_, P>`, set with a description and host samples | the store runs the function on a thread of its own, once per key, at full detail. Its outputs are High entries, and with `disk` also files. Evaluation waits for the job or reports it pending ([Evaluation](#evaluation)) |
| `Map(&MapBody)` | `#[node(map = body)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<[f32; N]>`, with a contract | the first `Point` is the mapped input, a description only. Other inputs are `Read`, materialized | the second `Point`, a description only. The output is a deferred image | the function returns the constants of `body`, a `#[cube] fn(px: Pixel, k: &[f32]) -> Pixel`. The first consumer that needs samples runs the segment in one kernel |
| `Warp` | `#[node(warp)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<Warp>`, with a contract | as for a map | as for a map | the function returns per-channel projective maps from the output plane to the input plane, in full-detail pixels, and a reconstruction filter, or none for a reindex. The maps compose into the segment's resampling stage |
| `Resample` | `#[node(resample)] fn(&Params, Point<In>, Point<Out>) -> Result<&'static FilterBody>`, with a contract | as for a map | as for a map | the function returns the filter. The contract's description sets the output lattice, and the stage resamples onto it |
| `Reduce(&ReduceBody)` | `#[node(reduce = BODY)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<[f32; N]>`, with a contract | as for a map | the second `Point`, a host value whose `Host` converts from the accumulator's `Vec<f32>` | the function returns the constants of `BODY`, and the node folds every pixel of its input with it ([Reductions](#reductions)) |
| `Bins(&BinsBody)` | `#[node(bins = BODY)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<[f32; N]>`, with a contract | as for a map | the second `Point`, a host value whose `Host` converts from the counts' `Vec<u32>`, `Counts` for every bins node so far | as for `Reduce`, counting pixels into the bins of `BODY` |
| `Meta` | `#[node(meta)] fn(&Params, &mut SubGraph, Inlet..., Outlet...) -> Result<()>`, without context or contract | `Inlet<'_, P, R>`: the input's description, checked against `R`, and a port the sub-graph's inputs read | `Outlet<P>`, set to an output of the sub-graph | the function builds a sub-graph of other kinds, which evaluation runs in the node's place ([Meta nodes](#meta-nodes)) |
| `Stencil(&StencilBody)` | `#[node(stencil = BODY)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<Stencil<N>>`, with a contract | as for a map | as for a map | the function returns the reach in full-detail pixels and the constants of `BODY`, a `#[cube] fn(w: &Window, k: &[f32]) -> Pixel`. The input materializes, and the stencil's kernel computes the samples a consumer asks for, with the maps after it ([Stencils](#stencils)) |

A `Write` allocates on first use: `host()` through `Payload::allocate`,
`device(ctx)` as `byte_len` uninitialized bytes. After the function returns,
the generated adapter publishes the output and rejects one written on both
sides or on neither. `Forward::set` publishes a raster the node holds,
usually an input, under the contract's description, sharing every
representation it has. Views and actions are kernels with inputs only and an
empty body.

Maps, warps, resamples and stencils read only `MapPayload`s, today `Rgb` as
packed `[f32; 3]`, whose `lattice` gives the stored extent and the full-detail
plane. A map keeps its input's lattice (`Image::map` fails otherwise). A
step on a deferred image extends its plan where [Segments](#segments)
allow, and otherwise starts a plan over that image. A step on a raster, or
on a deferred image already materialized, starts one. Kernel adapters
materialize their inputs, the adapters of the deferred shapes their `Read`
inputs, and evaluation the inputs it hands to frontends and the target of
`evaluate`. Steps after a fan-out each extend the shared plan and recompute
it in their own kernel. `MapBody` records the body's CubeCL expansion, its
constant count and a name unique in the binary. `FilterBody` records a
filter kernel `#[cube] fn(x: f32) -> f32`, its support and whether it
interpolates. `node/filter.rs` registers the five filters. The fused
kernel's identity is the stage kind, its filter and the body sequence, so
each combination compiles once whatever the constants.

### Segments

A plan composes a step only where the result equals running the nodes one
by one:

- A reindex, a warp taking every output sample onto an input sample
  (Orientation, an integer crop), commutes with per-pixel maps and joins the
  stage wherever it sits. `Warp::reindex` declares one, and evaluation
  checks it on the lattices.
- A filtering warp or a Resample before any map composes into the stage.
  After a map it cuts: the plan's output materializes on its own lattice and
  a new plan resamples it, two launches.
- A Resample fixes the stage's filter and lattice, so a filtering warp or
  Resample after it cuts too.
- A stencil is a stage of its own over its materialized input, so it always
  starts a plan, and any warp or Resample after it cuts. Maps after it join
  its plan.

At run time the stage loads samples when the composed map is the identity
on an equal lattice and the filter interpolates, gathers them when only
reindexes composed, and filters otherwise. A plan that neither moves nor
maps samples launches nothing and shares its input. The filtering path
scales the filter by the map's Jacobian per source axis, clamps footprints
at `MAX_FOOTPRINT` source samples, repeats edge samples for taps and makes
points outside the input, or beyond the map's horizon, black
([FIELD](FIELD.md)).

A plan computes any window of its output lattice. `Plan::input_window`
answers which part of the input a window reads: the same window through a
load, the reindexed corners through a gather, and through a filter the
corners mapped into the input, grown by the filter's support times the
footprint (`MAX_FOOTPRINT` for a projective map, whose footprint varies),
plus one sample, and through a stencil the window grown by its reach.
Projective maps keep lines straight, so the corners bound the window's
image, and a corner beyond the horizon asks for the whole input. A plan that
cuts asks its own input for that window in turn. The kernel positions
samples by their whole-lattice index and clamps taps to the stored window,
so a window's samples equal the same samples of the whole image bit for bit.

### Stencils

A stencil node's host function returns a `Stencil { radius, constants }`:
how far the body reads, in full-detail pixels of the input's plane, and the
constants of its `StencilBody` (`shape.rs`). The body is a `#[cube] fn(w:
&Window, k: &[f32]) -> Pixel` that reads `w.at(dx, dy)` for `|dx| <=
w.radius_x` and `|dy| <= w.radius_y`. The body's `border` rule answers
positions outside the lattice: `Border::Repeat` gives the nearest edge
sample, `Border::Zero` black. A stencil keeps its input's lattice.

Lengths are in full-detail pixels wherever a parameter states one, and the
compiler converts a stencil's reach to the samples of the lattice it reads:
`ceil(radius · extent / full)` per axis and at least one. `w.rate_x` and
`w.rate_y`, `extent / full`, convert the body's own lengths, so a preview
computes the same operation on its samples. A reach below one sample on an
axis of a preview lattice counts in `Statistics::approximated`. The
stencil then reads one sample on that axis, and the preview shows less of
its effect than full detail does.

A stencil reads samples around each output sample, so its input
materializes, and maps before it run in the input's own plan. The stencil
runs as one kernel with the maps after it. A crop of its output grows the
window by the reach, clamped to the lattice, crops the input to that, and
runs the stencil on the window, so a 100 % view through Sharpen computes the
crop and its halo only. The whole output is stored under the node's key and
a crop under `Key::region`, like any image. A stencil reads its input at
the requested scale, reduced by evaluation's box filter where it is finer,
and its shape is not scaled. Bayer stencils need windows aligned to CFA
cells, and none exists yet.

### Meta nodes

A meta node is one node in the graph a user edits and a sub-graph of other
kinds when it runs, as vkdt's `create_nodes` expands a module. Its function
receives a `SubGraph` (`expand.rs`), adds nodes to its `dag` with their
kinds' generated `add`, connects them with `Dag::connect`, feeds each
`Inlet` to sub-graph inputs with `SubGraph::input` and sets each `Outlet`
with `SubGraph::output`. `Node::expand` calls it with the input
descriptions, and `SubGraph::finish` rejects an unset output, a fed input
that is also connected, and foreign or meta nodes inside: foreign jobs
belong to the store under their own keys, and expansions do not nest.

Evaluation keys and schedules the meta node like any node. When it runs,
its inputs are resolved, so their descriptions exist even after a RAW
decode. The store keeps the 64 expansions used last by the meta node's key,
which covers its parameters and the keys of its inputs, so an unchanged
demand does not call the function again. Evaluation keys each sub-graph node
from the keys of the meta node's inputs, as if it were in the graph, takes
what the store holds walking back from the outputs, and runs the other
nodes in the sub-graph's order at the meta node's scale, each scaled shape
receiving that scale. Outputs the sub-graph materialized while it ran are
stored under their own keys, and the meta node's outputs under its own.
Errors name the meta node. A meta node reads its inputs at the requested
scale and its shape is not scaled. Its outputs are what the sub-graph's
nodes publish, often deferred, so crops and reductions pass through it.

A foreign node is the boundary for work that is expensive or outside
CubeCL: a decoder, a solver, host code over a whole image. Its function
sees no scale and runs on host samples, and the store memoizes it. Its
output descriptions exist only once it has run, as for a RAW, so it has no
contract and describes each output when it publishes it. `Cache::Disk`
holds a `Codec` per output, generated from the payload's `Persist`
implementation, and a payload without one does not compile there. RAW
decode (`raw.rs`) is a disk node and Hot pixel removal (`hot_pixels.rs`) a
memory one. Keys stamp the files on every evaluation, so a changed file is
a new key and decodes again. Tests bind data already in memory with
`node::source`. Files that a kernel's parameters name, such as output ICC
profiles, are assets: `#[node(assets)]` with `impl Assets for Params`, read
when an action or a preparation first asks and kept with the node, error
included.

`eval.rs` reads the shape to run foreign nodes as jobs and find their
codecs, to expand meta nodes, to derive each node's scale (`Shape::scaled`,
a claimed `scaled` kernel), and to hand frontends a reduction's outputs
(`Shape::reduces`). The macro builds every other difference into
`Node::identity`, `Node::contract` and `Node::run`, `Image` carries the
deferral, and `plan.rs` runs reductions.

### Reductions

A reduce or bins node turns its input into a small host value inside the
input's segment: the pass evaluates each pixel through the plan's stage and
map bodies, the same `#[cube]` function the fused store kernel calls, and
accumulates instead of storing. A histogram of the Sigmoid output therefore
launches no fused kernel and writes no image. The node's body is its only
device code. `plan.rs` derives the passes, so every reduction shares one
set of rules:

- A `ReduceBody` declares `identity`, the accumulator of
  `identity.len()` f32 before any pixel, `accumulate`, a `#[cube] fn(px:
  Pixel, k: &[f32], acc: &mut [f32])` that folds one pixel into `acc`, and
  `combine`, a `#[cube] fn(acc: &mut [f32], other: &[f32])` that folds one
  accumulator into another, associative and commutative. Each lane folds at
  most 256 pixels, strided across the image, then passes combine at most 256
  accumulators per lane until one remains, which is read back. The order is
  fixed and no float atomics run, so equal inputs give equal bits on one
  runtime.
- A `BinsBody` declares `bins` and 1 to 3 `channels` and a `#[cube] fn(px:
  Pixel, at: Position, k: &[f32]) -> Bin`, where `Position` is the pixel's
  offset in the window the pass covers and that window's extent, and `Bin`
  holds one bin index per channel. An index of `bins` or more counts
  nothing. Counts are u32, bin-major. Up to 4096 counts accumulate per
  workgroup in shared memory, one workgroup per 4096 pixels and at most 256
  workgroups, and a merge pass sums the workgroups' copies. Larger counts go
  to copies in global memory, at most 32 and together at most 2²⁰ counts,
  each lane adding a run of its pixels in one bin at once. The
  waveform's 196 608 counts get five copies and the vectorscope's 65 536
  get 16.
- Kernels with barriers take at most the runtime's lanes per workgroup,
  since the CPU runtime runs a host worker per lane.

The pass covers the samples the image holds: a deferred image's whole
lattice, or a raster's window. A reduction reads a deferred input in place
when it is the input's only consumer in the run. Otherwise evaluation
materializes the input first, once for all consumers, and the pass reads
the raster. Results are the same bits either way.

## Evaluation

Nothing runs until a caller forces it. A demand is a graph, a
`GlobalContext` whose `scale` requests preview detail (1 to 256, validated
at evaluation), and targets. `Evaluator::evaluate::<P>(dag, global, output)`
forces one output. It returns the raster where its node produced it, a
deferred target materialized on the device, and `Statistics`: nodes run,
each node of a meta node's sub-graph counted, uploads, downloads, fused
launches and the samples they wrote on the runtime, reduction passes
(`reductions`), stencil launches whose reach fell below one preview sample
(`approximated`), values the store served (`hits`), those of them read from
disk (`reads`) and representations it dropped (`evictions`) during the call.
`evaluate_inputs` forces several target nodes in one run and returns each
target's `InputValues`, which a frontend binds to the node's typed `Inputs`
with `bind::<N>()`. A reduce or bins target hands its outputs instead, bound
to the node's typed `Outputs` with `bind_outputs::<N>()`. They are demanded
like any value, so a stored one runs nothing. `evaluate_batch` takes
`Target`s, each a node and an optional region, runs under a `Policy`,
records host timings per node that runs and returns a `Batch` of the inputs
and the call's `Statistics`.

A region is a rectangle of the plane of the target's inputs in full-detail
pixels (`shape::Rect`). Each input on a plane then holds only its samples
whose cells meet the region, as a raster whose description states its
window, and the fused kernels compute only those samples and what they
read. A region covering the plane is the whole image. Other inputs are
whole. Only a node without outputs, a view or an action, takes a region, so
a reduction target does not: its outputs would be keyed as the whole
image's.
Kernels, foreign nodes and sources run on whole images, so a region bounds
the work of the segments between the last of them and the target. A
stencil's segment reads its region grown by the reach, so regions pass
through stencils.

A call first decides the scale each needed node runs at. From the targets
every node is read at the requested scale, and a foreign node reads its
inputs at scale 1, so a foreign node runs on full-detail data wherever it
sits. A node read at the requested scale runs at it when its outputs depend
on it: its shape is scaled, or it reads an image on a plane, which arrives
reduced or from a node that runs at the scale. Every other node runs at
scale 1, so the RAW, its gains and highlight reconstruction are keyed and
stored once for every preview level. Where a node reads an image on a plane
at a scale coarser than 1 and its lattice is finer than `div_ceil(full,
scale)`, evaluation reduces it with the box filter `shape::BOX` as a
resampling stage of the consumer's plan, unless a `scaled` kernel produced
it or the consumer's shape is scaled. An RGB source feeding maps at scale 4
is therefore box-reduced and mapped in one launch, and Orientation of a
full-detail image is reduced after it. The box reduction is a preview
approximation and never applies at scale 1.

The call then keys every node it may run, in `Dag::order`, the topological
order recomputed on every graph edit, which runs siblings by node index.
Keying a foreign node stamps its files. It then walks back from the demanded
values. A value stored in memory, or on disk for a `foreign = disk` node, is
a hit, and a missing one demands the value it derives from, a box reduction
or a region, or schedules its node and the values that node reads. Target
nodes always run, so their contracts check what they receive, except reduce
and bins targets, whose outputs are demanded. The scheduled nodes run once
each per scale in graph order. The run counts the consumers of each value
and drops its reference after the last. The store then keeps the value if it
has a key and samples. A Sigmoid-only edit therefore runs the five nodes of
the tail segment and one fused launch from the stored demosaic output, and
an unchanged demand runs no node. A target's inputs are materialized before
the run ends, so the store keeps them, except an input on a plane larger
than the device budget, which `InputValues` materializes when bound or
computes by region (`InputValues::region`, `InputValues::bands`).

Per node, evaluation hands the input descriptions to `Node::contract`. The
generated contract fails an unconnected required input with `MissingInput`
and checks each connected input against its port requirement. It then calls
the node's contract function with `&GlobalContext`, the parameters, `&Desc`
per required input and `Option<&Desc>` per optional one, and validates each
returned description with `Payload::validate` and the output's requirement.
`Node::run` receives a `KernelContext` (global context and runtime) and the
inputs. A node whose shape is scaled receives the scale it runs at, and
every other node scale 1. Host outputs are checked with `Payload::check`,
device and forwarded outputs against `byte_len`. The node's inputs are
released after it runs. For a meta node evaluation calls `Node::expand`
instead, then the contract and run of each node of its sub-graph.

A foreign node's contract checks its inputs and describes nothing. The run
materializes its inputs, then asks the store for the job under the node's
key, which starts one if none is running or failed. The job's thread
downloads the inputs, calls the function with scale 1 and publishes what it
set. Under `Policy::Wait`, which `evaluate` and `evaluate_inputs` use, the
run blocks until the job ends. Under `Policy::Defer` a running job fails the
node with `Error::Pending`, which reaches its consumers like any failure, and
the job keeps running. When a job ends it writes its disk files, then calls
the callback given to `Evaluator::with_wake` on its thread. The next run
moves its outputs into the store, so demanding the same values again hits.

Graph edits check what ports alone determine: equal payload types, one
source per input and no cycle (`graph.rs`). Admission checks parameters
(`#[validate(fn)]`). Contracts and requirements read descriptions that exist
only once data is loaded, so they run when evaluation forces the node and
fail on it.

`evaluate` stops at the first error. `evaluate_batch` records a failure as
`Error::Node` with the node's ID and kind. Nodes downstream report the same
error, naming the failed ancestor, and targets whose ancestors succeeded
still receive their inputs. A branch nothing forces reports nothing. Each
call ends with one device wait (`RuntimeContext::finish`), and a device
error there fails every result.

The GUI worker (`drip-gui/src/worker.rs`) owns one `Evaluator` per project
and runs demands in order on one thread. A preview demand forces the nodes
whose binding prepares a view, at scale `2^level` with level 0 to 8, then
prepares each view from its inputs, or a scope from its counts. A newer
preview request replaces a pending one and makes a running one's result
stale. A detail demand prepares one view from its input over a region at
scale 1. The image view navigates in full-detail pixels, and at 100 % or
more over a coarser preview its window asks for the visible region. The
worker sends the latest region per view after any waiting preview and draws
the crop over the magnified preview once both belong to the same graph, so
an edit shows the new preview alone until its crop arrives. Scopes keep the
preview's samples. An action demand forces one node at full detail and runs
its action. Demands are not merged, but a later demand reuses what the store
kept from earlier ones. A new project replaces the worker, its evaluator and
the memory store. Every worker's evaluator has the disk tier of
`Disk::platform()`, so a RAW decoded in one session is read from disk in the
next.

Previews evaluate under `Policy::Defer` and actions under `Policy::Wait`. A
target waiting for a foreign job keeps its previous view, the worker stays
busy, and the status line reads "Computing *node*…" in place of
"Evaluating…", logged at debug level rather than as a failure. The
evaluator's wake callback sends the worker a ready event, and the worker
sends its latest preview request again, which finds the job's outputs
stored.

## Data model

Payloads and their interpretations live in `node/data.rs`.

- `Rgb`: packed f32 RGB, row-major, `HostBuffer<[f32; 3]>`, described by
  `RgbDesc { extent, full, window, coordinates }`. `extent` counts the
  stored samples and `full` is the extent of the same image at full detail.
  A raster of the whole image has no `window`, and its pixel `i` sits at
  full-detail position `(i + 0.5) · full / extent`. A crop or an export band
  states `window`: the lattice index of its first sample and the whole
  lattice's extent, so its pixel `i` is lattice sample `origin + i`.
  Contracts describe whole images. Validation rejects a lattice of more
  samples than `full`, a window outside its lattice and a window of the
  whole lattice, which is none.
- `Bayer`: one f32 per sensor site, row-major, `HostBuffer<f32>`, described
  by `BayerDesc { extent, phase, response }`.
- `BayerLevels`: clipping levels of the four local sites, `[f32; 4]`, with
  phase and response.
- `CaptureMetadata`: decoded capture facts and native calibration, host
  only. Description and value are one `Arc<drip_raw::Metadata>`, so
  contracts read the facts. Validation requires an EXIF orientation from 1
  to 8.
- `Counts`: a bins node's u32 counts, bin-major, `HostBuffer<u32>`, host
  only, described by `CountsDesc { bins, channels, coordinates }`, the
  coordinates of the RGB samples counted.

`Rgb`, `Bayer` and `CaptureMetadata` implement `Persist`, the stream the
disk tier keeps: the description as length-prefixed JSON, then the samples'
bytes, or for metadata the facts as JSON. Metadata holding a non-finite
float does not read back, so it is decoded again instead.

Samples are linear. `Coordinates` says what three channels mean:
`Camera(Response)`, with the camera's make and model and the white-balance
gains already applied (`balance`), or `Space(RgbSpace)`, with xy primaries
and white. Spaces compare exactly, so only values copied from `REC2020_D65`
count as Rec.2020/D65.

A requirement is a port type argument implementing `Requirement<P>`:
`Rec2020` and `CameraRgb` for `Rgb`, and `Any` by default. On an input it is
checked before the contract, on an output it is a guarantee checked after.
Its `LABEL` follows the payload name in help and on the canvas.

## Module map

In `crates/drip/src`, the modules outside `node/` are infrastructure and
name no concrete node or payload, except `registry.rs`, which lists the
kinds by hand. `node/` holds the nodes, payloads, colour management,
resampling filters and shared kernels. `crates/drip-macros` generates node
adapters (`#[node]`) and parameter schemas (`Parameters`, `Choice`).
`crates/drip-raw` wraps LibRaw. The file lists are in the
[drip README](../crates/drip/README.md#files) and the
[drip-gui README](../crates/drip-gui/README.md#files).

## Adding a node

A kernel is a function, a contract and Rustdoc help in `node/<name>.rs`,
declared in `node/mod.rs` and listed in `registry::NODES`. The
[README example](../crates/drip/README.md#adding-a-node) doubles RGB
samples. `reduce/rgb.rs` is the closest real node: one device kernel and
validated parameters. `bayer_gains.rs` writes one output on the device and
one on the host. A kernel receives scale 1 and whole inputs, already reduced
when they are RGB at a preview scale. Declare `#[node(scaled)]` only for a
kernel that reaches the requested scale itself, better or cheaper than
evaluation's box reduction, as `rcd/mod.rs` and `bayer_2x2.rs` do through
`reduce::for_demosaic`: its contract then describes its output at
`GlobalContext::scale`, and its outputs are keyed by the scale.

A per-pixel operation on `Rgb` is a map: a host function that turns
parameters and descriptions into constants, and a `#[cube]` body over one
`Pixel`. The README's second example is the map form of the same node.
`exposure.rs` is the smallest real map, and `camera_to_rgb.rs` also reads a
`Read` input on the host to build its matrix.

An operation on a neighbourhood of `Rgb` samples is a stencil: a host
function that returns the reach and constants, a `static StencilBody` with a
name unique among stencil bodies, and its `#[cube]` body. `sharpen.rs` is the
one stencil. State lengths in full-detail pixels, return the reach that
covers the body's taps, and scale lengths inside the body by the window's
rate.

An operation built from other nodes, such as a pyramid, is a meta node:
`#[node(meta)]` on a function that wires a `SubGraph` to its `Inlet` and
`Outlet` ports. `pyramid.rs` builds Pyramid blur from Resample and Sharpen
and reads its input's full extent from the `Inlet` to size the last
Resample.

Outputs are stored under keys that include `Kind::version`. When a change
alters what a kind computes for the same parameters and inputs, bump it with
`#[node(version = N)]` (1 by default). RAW decode uses
`drip_raw::VERSION`, so a LibRaw update invalidates stored decodes. Kernels
must be pure: a kernel that reads a file keeps returning the stored value
until its parameters change, since only foreign nodes stamp their files.

A measurement over every pixel is a reduction. Declare a `static
ReduceBody` or `BinsBody` with a name unique among its kind and its
`#[cube]` bodies beside the node, and a node `#[node(reduce = BODY)]` or
`bins = BODY` whose host function returns the body's constants. Its
contract describes the output. The scopes in `drip-gui/src/node_ui/scopes/`
are bins nodes. A frontend can also call `Image::fold` or `Image::count`
directly, as `exposure::meter` does for auto exposure with its
`MEAN_LUMINANCE` body.

A geometric operation is a warp: its host function returns a `Warp` with a
map per channel from the output plane to the input plane, both in
full-detail pixels (`[0, full]`), and its contract describes the output's
lattice. `orientation.rs` returns a reindex, and `perspective.rs` a
projective map filtered with Lanczos-3, with the host solvers its GUI view
calls. A warp that only reorders samples uses `Warp::reindex` so it can move
across maps. `resample.rs` is the one resample.

Work that runs on the host over whole images, or outside CubeCL, is a
foreign node: `#[node(foreign = memory)]` or `foreign = disk`, no context,
`Read` inputs whose host samples it reads, and `Publish` outputs it sets
with a description and host samples. `hot_pixels.rs` is the smallest. A node
that reads files names them with `Path` parameters, which key it, and fails
with an error naming the missing selection or file, as `raw.rs` does. The
node stays editable. Declare `disk` when running the node costs more than
reading its outputs, and implement `Persist` for each output payload.

A node with an action has an empty body, inputs only and `action = fn`,
where `fn(&Params, &InputValues)` binds the inputs itself. `export.rs`
declares Export TIFF this way, with `assets` for its output profile. Its
action asks `InputValues::bands` for bands of rows within the device budget
and binds `InputValues::region` per band, so the fused tail's output over
the budget never lives whole on the device, and writes bytes equal to a
single-piece export.
Views that only the GUI shows are declared in drip-gui.

Unit tests sit in the node's file. Graph tests in `crates/drip/tests/` build
a `Dag`, bind inputs with `node::source` and evaluate. Device kernels belong
in `tests/algorithms.rs`, with the resampling reference in
`tests/algorithms/resample.rs`, segments in `tests/fusion.rs`, the store in
`tests/store.rs`, foreign jobs and the disk tier in `tests/foreign.rs`,
regions, scale and tiled export in `tests/regions.rs`, reductions in
`tests/reduce.rs`, stencils in `tests/stencil.rs`, and meta nodes in
`tests/meta.rs`. A test that counts
the work of one evaluation uses a new `Evaluator`, whose store is empty. A
test with a disk tier gives it a temporary directory. Each file runs its
checks on the CPU runtime with `--features drip/cpu` and on the
`DRIP_BACKEND` runtime with `cargo test -p drip -- --ignored
selected_backend`.

Help is the Rustdoc on the node function: operation, assumptions, inputs
and outputs by payload name, creative reinterpretations. Papers and sources
go in `#[node(references = [(label, url), ...])]`. Credit adapted code
beside it and in `THIRD_PARTY.md`.

## GUI binding

Every listed kind appears in the editor with ports, help and schema controls
from its `Kind`. Node-specific presentation is an `impl GuiNode` on the
node's generated `Definition`, listed in `BINDINGS` in
`drip-gui/src/registry.rs`. `controls` replaces the schema controls,
`PREPARE` turns what a preview hands the node into a drawable view, and
`ACTION` adds a frontend action whose result may become new parameters.
`Prepare::Inputs` prepares from the node's typed inputs and
`Prepare::Outputs` from a reduction's typed outputs. `PREPARE` also
prepares a detail, the view of a region of the inputs at full detail, whose
inputs state their window. Both receive a `PrepareContext` with the
demand's `GlobalContext` and the evaluator's runtime, so they read device
samples and run drip kernels where the inputs live. Perspective Transform's
view resamples its input through `Image::warp` and `Image::raster` rather
than a kernel of its own, and auto exposure calls `exposure::meter`.
drip-gui launches no kernel of its own.

Preview, Histogram, Waveform and Vectorscope are declared in drip-gui with
`#[drip::node]`, listed in `VIEWS` and tagged `view`, the scopes also
`scope`. Preview is a view of its input. The scopes are bins nodes whose
only device code is their bin body: their `Counts` output, shown as a port,
is stored like any value, and their views plot it. A view
whose window edits its node implements `Drawable::interact` and sets
parameters through `ControlCx::set`, as Perspective Transform's view does.
The [drip-gui README](../crates/drip-gui/README.md#presenting-a-node) walks
through an example.

## Migration status

Drip adopted the compute model of
[COMPUTE_MODEL](COMPUTE_MODEL.md#proposal-for-drip) in eight steps, all of
which landed on 2026-10-13. Step 1 made ports name
payloads, values `Image` and `Raster`, gave kinds a `Shape` and moved a
run's values into a `Store`. Step 2 added the map shape and fused kernels
and made Exposure, Camera RGB to color RGB, Chromatic adaptation and Sigmoid
maps. Step 3 added warps, Resample and the resampling stage, replacing
`RgbField`, Interpolate and Sample, and made Perspective Transform and
Orientation warps. Step 4 made the store a memo with content keys,
durability and per-side budgets. Step 5 added foreign nodes, the pending
policy and the disk tier, made RAW decode a disk node in place of the source
form, and added Hot pixel removal. Step 6 added regions and windows to the
fused segments, derived each node's scale from shapes with the `scaled`
claim and evaluation's box reduction, made Export TIFF write in bands over
the device budget and gave the GUI's image view full-detail crops at 100 %.
Kernels stay whole, without halos. Step 7 added the reduce and bins shapes,
made the scopes bins nodes and auto exposure a library reduction, and
removed drip-gui's kernels. Step 8 added the stencil shape with Sharpen and
the meta shape with Pyramid blur.
