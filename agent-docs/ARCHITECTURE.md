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
`Host` and a description type `Desc`. A description holds what a contract
reads without touching samples: extent, CFA phase, colour coordinates. Payloads that kernels read also implement `DeviceLayout`,
which fixes their byte layout on the device.

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

A deferred image holds an input image and a `Plan` (`plan.rs`): one
resampling stage, per-channel projective maps from the image's plane to the
input's plane with their filter, then the map bodies to apply and their
constants. The input is a raster, or a deferred image where the plan cuts
(see [Segments](#segments)). `Image::materialize` materializes the input,
runs the plan once as one fused kernel into a device raster and drops the
plan and its input, so a segment never stores its intermediate images.
Later calls and clones share the result. `Image::raster` materializes and
returns the samples, for frontends.

The `Store` (`store.rs`) holds the outputs of one evaluation, keyed by
output port, with the number of consumers still to run. An output is dropped
after its last consumer, so peak memory follows the live edges. An
evaluation leaves behind only the result its caller holds and the files that
sources and assets keep.

## Shapes

`Kind::shape` (`shape.rs`) declares how evaluation runs a kind. `#[node]`
derives it from the declaration: `Load` ports make a source, `map = body`,
`warp` or `resample` with two `Point` ports make a map, warp or resample,
and any other function is a kernel.

| Shape | Function | Inputs | Outputs | Evaluation |
|---|---|---|---|---|
| `Kernel` | `fn(&KernelContext, &Params, ports...) -> Result<()>`, with a contract when it has outputs | `Read<'_, P, R>`, or `Option<Read<..>>` when optional | `Write<'_, P, R>`, written on host or device, or `Forward<'_, P, R>`, set from a raster the node holds | the contract, then the function on materialized inputs, on every evaluation that forces it |
| `Source` | `fn(&Params, Load<'_, P>...) -> Result<()>`, without context or contract | none | `Load<'_, P>`, set with a description and host samples | loads when first forced and keeps the outputs or the error. Each evaluation receives fresh rasters sharing the host samples |
| `Map(&MapBody)` | `#[node(map = body)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<[f32; N]>`, with a contract | the first `Point` is the mapped input, a description only. Other inputs are `Read`, materialized | the second `Point`, a description only. The output is a deferred image | the function returns the constants of `body`, a `#[cube] fn(px: Pixel, k: &[f32]) -> Pixel`. The first consumer that needs samples runs the segment in one kernel |
| `Warp` | `#[node(warp)] fn(&Params, Point<In>, Read..., Point<Out>) -> Result<Warp>`, with a contract | as for a map | as for a map | the function returns per-channel projective maps from the output plane to the input plane, in full-detail pixels, and a reconstruction filter, or none for a reindex. The maps compose into the segment's resampling stage |
| `Resample` | `#[node(resample)] fn(&Params, Point<In>, Point<Out>) -> Result<&'static FilterBody>`, with a contract | as for a map | as for a map | the function returns the filter. The contract's description sets the output lattice, and the stage resamples onto it |

A `Write` allocates on first use: `host()` through `Payload::allocate`,
`device(ctx)` as `byte_len` uninitialized bytes. After the function returns,
the generated adapter publishes the output and rejects one written on both
sides or on neither. `Forward::set` publishes a raster the node holds,
usually an input, under the contract's description, sharing every
representation it has. Views and actions are kernels with inputs only and an
empty body.

Maps, warps and resamples read only `MapPayload`s, today `Rgb` as packed
`[f32; 3]`, whose `lattice` gives the stored extent and the full-detail
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

At run time the stage loads samples when the composed map is the identity
on an equal lattice and the filter interpolates, gathers them when only
reindexes composed, and filters otherwise. A plan that neither moves nor
maps samples launches nothing and shares its input. The filtering path
scales the filter by the map's Jacobian per source axis, clamps footprints
at `MAX_FOOTPRINT` source samples, repeats edge samples for taps and makes
points outside the input, or beyond the map's horizon, black
([FIELD](FIELD.md)).

A source keeps its loaded outputs, or its failure, in a `OnceLock` in the
admitted node. Files are assumed not to change. Editing parameters admits a
new node, which reads again. A fresh raster per evaluation keeps a device
upload of loaded samples with that evaluation. Tests bind data already in
memory with `node::source`, which is a source too. Files that a kernel's
parameters name, such as output ICC profiles, are assets:
`#[node(assets)]` with `impl Assets for Params`, read when an action or a
preparation first asks and kept the same way.

`eval.rs` does not read the shape. The macro builds the difference into
`Node::contract` and `Node::run`, and `Image` carries the deferral.

## Evaluation

Nothing runs until a caller forces it. `Evaluator::evaluate::<P>(dag,
global, output)` forces one output. It returns the raster where its node
produced it, a deferred target materialized on the device, and `Statistics`:
nodes run, and uploads, downloads and fused launches on the runtime during
the call. `evaluate_inputs` forces several target nodes in one run and
returns each target's materialized `InputValues`, which a frontend binds to
the node's typed `Inputs` with `bind::<N>()`. `evaluate_batch` does the same
and records host timings per node.

Evaluation runs the targets and their ancestors once each, in `Dag::order`,
the topological order recomputed on every graph edit.
`GlobalContext::scale` requests preview detail and is validated at
evaluation (1 to 256). RCD and Bayer 2×2 demosaic reduce each CFA phase
plane by it before demosaicing, Perspective Transform and Resample store
`div_ceil(full, scale)` samples, and other nodes keep their input's
sampling.

Per node, evaluation hands the input descriptions to `Node::contract`. The
generated contract fails an unconnected required input with `MissingInput`
and checks each connected input against its port requirement. It then calls
the node's contract function with `&GlobalContext`, the parameters, `&Desc`
per required input and `Option<&Desc>` per optional one, and validates each
returned description with `Payload::validate` and the output's requirement.
`Node::run` receives a `KernelContext` (global context and runtime) and the
inputs. Host outputs are checked with `Payload::check`, device and forwarded
outputs against `byte_len`. The node's inputs are released after it runs.

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
prepares each view from its inputs. A newer preview request replaces a
pending one and makes a running one's result stale. An action demand forces
one node at full detail and runs its action. Demands are not merged.

## Data model

Payloads and their interpretations live in `node/data.rs`.

- `Rgb`: packed f32 RGB, row-major, `HostBuffer<[f32; 3]>`, described by
  `RgbDesc { extent, full, coordinates }`. `full` is the extent of the same
  image at full detail. Stored pixel `i` sits at full-detail position
  `(i + 0.5) · full / extent`. Validation rejects storing more samples than
  `full`.
- `Bayer`: one f32 per sensor site, row-major, `HostBuffer<f32>`, described
  by `BayerDesc { extent, phase, response }`.
- `BayerLevels`: clipping levels of the four local sites, `[f32; 4]`, with
  phase and response.
- `CaptureMetadata`: decoded capture facts and native calibration, host
  only. Description and value are one `Arc<drip_raw::Metadata>`, so
  contracts read the facts. Validation requires an EXIF orientation from 1
  to 8.

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
one on the host.

A per-pixel operation on `Rgb` is a map: a host function that turns
parameters and descriptions into constants, and a `#[cube]` body over one
`Pixel`. The README's second example is the map form of the same node.
`exposure.rs` is the smallest real map, and `camera_to_rgb.rs` also reads a
`Read` input on the host to build its matrix.

A geometric operation is a warp: its host function returns a `Warp` with a
map per channel from the output plane to the input plane, both in
full-detail pixels (`[0, full]`), and its contract describes the output's
lattice. `orientation.rs` returns a reindex, and `perspective.rs` a
projective map filtered with Lanczos-3, with the host solvers its GUI view
calls. A warp that only reorders samples uses `Warp::reindex` so it can move
across maps. `resample.rs` is the one resample.

A source declares `Load` ports and no context, and calls `set` on each with
a description and host samples, as `raw.rs` does after decoding. Fail with
an error naming the missing selection or file. The node stays editable.

A node with an action has an empty body, inputs only and `action = fn`.
`export.rs` declares Export TIFF this way, with `assets` for its output
profile. Views that only the GUI shows are declared in drip-gui.

Unit tests sit in the node's file. Graph tests in `crates/drip/tests/` build
a `Dag`, bind inputs with `node::source` and evaluate. Device kernels belong
in `tests/algorithms.rs`, with the resampling reference in
`tests/algorithms/resample.rs`, and segments in `tests/fusion.rs`. Each file runs
its checks on the CPU runtime with `--features drip/cpu` and on the
`DRIP_BACKEND` runtime with `cargo test -p drip -- --ignored selected_backend`.

Help is the Rustdoc on the node function: operation, assumptions, inputs
and outputs by payload name, creative reinterpretations. Papers and sources
go in `#[node(references = [(label, url), ...])]`. Credit adapted code
beside it and in `THIRD_PARTY.md`.

## GUI binding

Every listed kind appears in the editor with ports, help and schema controls
from its `Kind`. Node-specific presentation is an `impl GuiNode` on the
node's generated `Definition`, listed in `BINDINGS` in
`drip-gui/src/registry.rs`. `controls` replaces the schema controls,
`PREPARE` turns the node's typed inputs into a drawable view, and `ACTION`
adds a frontend action whose result may become new parameters. Both receive
a `PrepareContext` with the demand's `GlobalContext` and the evaluator's
runtime, so they read device samples and run drip kernels where the inputs
live. Perspective Transform's view resamples its input through
`Image::warp` and `Image::raster` rather than a kernel of its own.

Views that only present their inputs, Preview and the scopes, are declared
in drip-gui with `#[drip::node]`, listed in `VIEWS` and tagged `view`. A view
whose window edits its node implements `Drawable::interact` and sets
parameters through `ControlCx::set`, as Perspective Transform's view does.
The [drip-gui README](../crates/drip-gui/README.md#presenting-a-node) walks
through an example.

## Migration status

Drip is adopting the compute model of
[COMPUTE_MODEL](COMPUTE_MODEL.md#proposal-for-drip) in eight steps. Steps 1
to 3 landed on 2026-10-13. Step 1 made ports name payloads, values `Image`
and `Raster`, gave kinds a `Shape` and moved a run's values into a `Store`.
Step 2 added the map shape and fused kernels and made Exposure, Camera RGB
to color RGB, Chromatic adaptation and Sigmoid maps. Step 3 added warps,
Resample and the resampling stage, replacing `RgbField`, Interpolate and
Sample, and made Perspective Transform and Orientation warps. Step 4 is
next: content keys, durability and the device budget.
