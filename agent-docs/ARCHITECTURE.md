# Architecture

How Drip is built, as of 2026-10-13. Read this first, then the crate
documentation (`cargo doc --no-deps -p drip -p drip-gui --open`) for the
rules each module implements. [DESIGN](DESIGN.md) records why,
[TODO](TODO.md) what is unfinished, and [PROGRESS](PROGRESS.md) the
measurements. [COMPUTE_MODEL](COMPUTE_MODEL.md), [FIELD](FIELD.md) and
[TYPE_SYSTEM](TYPE_SYSTEM.md) are research behind the design.

| Crate | Role |
|---|---|
| `drip` | headless library: node graph, processing, colour management, persistence |
| `drip-macros` | `#[node]`, `Parameters` and `Choice`, re-exported by `drip` |
| `drip-raw` | bundled LibRaw and its safe `decode` |
| `drip-gui` | interactive frontend on winit, wgpu and egui |
| `drip-cli` | batch frontend, a scaffold for now |

## Modules

The `drip` library splits into what a node is written against and what
runs it. The [drip README](../crates/drip/README.md#files) lists every file.

```text
node-facing    ports  payload  body  lattice  runtime  expand::SubGraph
               definition::Assets  eval::InputValues  registry::tag
               ───────────────────────────────────────────────────────
evaluation     shape  plan  eval  store  image  graph  definition  registry
```

A node file names only the node-facing modules above, the `Parameters`
trait of its own parameters, and the shared image code in `node/`, such as
`data.rs`, `color.rs` and `filter.rs`. A payload on a plane also names
`image::Plane`, which `Payload::PLANE` holds. Files outside `node/` name no
concrete node or payload, except `registry.rs`, which lists the kinds.

## Values

A node computes context × parameters × inputs → outputs. Each port names a
payload, a Rust type implementing `Payload` with a host value and a
description. A description holds what a contract reads without touching
samples: extent, CFA phase, colour coordinates.

The value on an edge is an `Image`. It is either a `Raster`, samples that
exist on the host, the device or both, or a deferred image that a map,
warp, resample or stencil published and nobody has computed yet:

```text
  kernel, foreign node                    map, warp, resample, stencil
          │                                          │
          ▼                                          ▼
      Raster<P> ◄────── materialize ─────── deferred Image
   ┌──────┴──────┐      one fused kernel    input Image + Plan
  host ◄──────► device                               │
  each side made on first request,                   └──► fold, count:
  transfers counted                                       host values, nothing stored
```

A node asks each input for the side it needs, so signatures never name a
placement. Rasters are immutable and shared, so fan-out and forwarded
outputs cost nothing and nodes stay pure. A deferred image materializes
once, when the first consumer needs samples, and its clones share the
result.

### Payloads

Payloads and their interpretations live in `node/data.rs`:

| Payload | Samples | Description |
|---|---|---|
| `Rgb` | packed `[f32; 3]`, row-major | `RgbDesc { extent, full, window, coordinates }` |
| `Bayer` | one f32 per sensor site | `BayerDesc { extent, phase, response }` |
| `BayerLevels` | clipping levels of the four local sites | phase and response |
| `CaptureMetadata` | decoded capture facts, host only | the same shared value |
| `Counts` | a bins node's u32 counts, host only | `CountsDesc { bins, channels, coordinates }` |

Samples are linear. `Coordinates` says what three channels mean: camera
responses with the white balance already applied, or an RGB space by its
primaries and white. Spaces compare exactly, so only values copied from
`REC2020_D65` count as Rec.2020/D65.

`Rgb` sits on a lattice over the image's full-detail plane: `extent`
samples cover `full` pixels, and sample `i` sits at
`x_plane = (i + 0.5) · full / extent`. A preview stores fewer samples of the
same plane. A crop or an export band states a `window` of that lattice.
Contracts always describe whole images.

A requirement is a port type argument, `Read<'_, Rgb, Rec2020>`, checked on
the input's description before the contract and on the output's after it.
Help and the canvas show it after the payload name.

## Shapes

Every kind declares a shape, which `#[node]` derives from the declaration.
The shape tells evaluation how to run the node:

| Shape | Declaration | Function returns | Evaluation |
|---|---|---|---|
| Kernel | `#[node(contract = f)]` | `()`, after writing its outputs | runs on whole materialized inputs |
| Foreign | `foreign = memory` or `disk` | `()`, after publishing its outputs | the store runs it on a thread of its own, at full detail, once per key |
| Map | `map = body` | the constants of a `#[cube]` pixel body | fused with neighbouring steps into one kernel |
| Warp | `warp` | projective maps from output to input plane, and a filter | composed into the segment's resampling stage |
| Resample | `resample` | a filter | resamples once onto the contract's lattice |
| Stencil | `stencil = BODY` | a reach in full-detail pixels and constants | reads a window of its materialized input, fused with the maps after it |
| Reduce, Bins | `reduce = BODY`, `bins = BODY` | the body's constants | folds or counts every pixel inside the input's segment |
| Meta | `meta` | `()`, after wiring a `SubGraph` | runs the sub-graph's nodes in its place |

Kernels, foreign nodes and bound sources are opaque and run on whole
images. The deferred shapes let the segment compiler (`plan.rs`) run a chain
such as Orientation, Exposure and Sigmoid as one kernel that writes only the
last image, and compute just the window a consumer asks for. It composes a
step only where the result equals running the nodes one by one. The rules
are in `plan.rs`.

## Evaluation

Nothing runs until a caller forces it. A graph edit checks only what ports
determine: payload types, one source per input and no cycle. Contracts read
descriptions that exist only once data is loaded, so they run when
evaluation forces the node, and fail on it.

```text
demand ─► scale per node ─► key per value ─► Store ─┬─ hit ─────────────┐
(targets,   full detail      SHA-256 of kind,       │                   ▼
 scale,     unless the       parameters, inputs     └─ miss ─► run ─► materialize
 regions)   output depends                                   in graph   for each consumer,
            on the scale                                     order      then keep in Store
```

1. **Scale.** A preview requests a scale. Only nodes whose outputs depend on
   it run at it, so a RAW decode and its gains are computed once for every
   preview level, and a full-detail image read at a preview scale is
   box-reduced inside its consumer's kernel.
2. **Keys.** Every value gets a content key. Equal keys mean equal samples.
3. **Store.** A stored value is a hit. A missing one schedules its node.
4. **Run.** Scheduled nodes run in graph order. Deferred images
   materialize at the first consumer that needs samples, over a target's
   region only when it has one.
5. **Keep.** The store keeps materialized values within a byte budget per
   side, evicting the least durable and least recently used first. Foreign
   outputs declared `disk` also survive the process.

A Sigmoid-only edit therefore runs the tail segment as one launch from the
stored demosaic output, and an unchanged demand runs no node. The rules
live in `eval/mod.rs` (scale, keys, regions, policies, errors) and `store.rs`
(keys, eviction, jobs, disk).

`Evaluator::evaluate` forces one output. `evaluate_batch` forces several
targets, each with an optional region, under a `Policy`: `Wait` blocks on a
running foreign job, and `Defer` reports the node pending so a preview can
return. A frontend binds a target's inputs, or a reduction's outputs, to the
node's typed views with `InputValues::bind` or `bind_outputs`.

## Adding a node

The drip README has compiled examples of most shapes. Put the node in
`node/<name>.rs`, declare it in `node/mod.rs` and list its `KIND` in
`registry::NODES`.

- **Kernel.** A function, a contract and help. It receives scale 1 and
  whole inputs, already reduced when they are RGB at a preview scale.
  Declare `scaled` only when it reaches the requested scale better or
  cheaper than the box reduction, as the demosaics do. Examples:
  `reduce/rgb.rs` (one device kernel), `bayer_gains.rs` (one output on the
  device, one on the host), `rcd/mod.rs` (scaled).
- **Map.** A per-pixel operation on `Rgb`: a host function that turns
  parameters and descriptions into constants, and a `#[cube]` body over one
  `Pixel`. Examples: `exposure.rs`, and `camera_to_rgb.rs`, which also
  reads its metadata input.
- **Warp.** A geometric operation: maps per channel from the output plane
  to the input plane in full-detail pixels, and a filter. A warp that only
  reorders samples returns `Warp::reindex`, so it moves across maps.
  Examples: `orientation.rs` (reindex), `perspective.rs` (Lanczos-3).
- **Resample.** `resample.rs` is the one resample: its contract sets the
  output lattice, and its function picks the filter.
- **Stencil.** A neighbourhood operation: the reach and constants, and a
  `static StencilBody` with a name unique among stencils. State lengths in
  full-detail pixels and scale them inside the body by the window's rate.
  Example: `sharpen.rs`.
- **Reduce, Bins.** A measurement over every pixel: a `static ReduceBody`
  or `BinsBody` with a unique name, and a contract describing the host
  output. Examples: the scopes in `drip-gui/src/node_ui/scopes/`, and
  `exposure::meter`, which calls `Image::fold` without a node.
- **Foreign.** Work over whole images on the host, or outside CubeCL:
  `Read` inputs and `Publish` outputs. Files it reads are `Path`
  parameters, stamped into its key. Declare `disk` when running costs more
  than reading the outputs back. Examples: `hot_pixels.rs`, `raw.rs`.
- **Meta.** An operation built from other nodes: wire a `SubGraph` to the
  `Inlet` and `Outlet` ports. Sub-graphs hold no foreign or meta node.
  Example: `pyramid.rs`.
- **Action.** A node with inputs only, an empty body and `action = fn`,
  such as Export TIFF in `export.rs`. Its action computes images over the
  device budget in bands with `InputValues::bands` and `region`.

Keep kernels pure. Only foreign nodes stamp the files they read, so a
kernel that read a file would keep returning its stored output until its
parameters changed. Files a kernel's parameters name, such as output ICC
profiles, are assets: `#[node(assets)]`, read on first use and kept with
the node. Bump `#[node(version = N)]` when a change alters what a kind
computes for the same parameters and inputs.

Help is the Rustdoc on the node function, in the shape of darktable's
module reference: operation, assumptions, inputs and outputs by payload
name, creative reinterpretations. Sources go in
`#[node(references = [(label, url), ...])]`. Credit adapted code beside it
and in `THIRD_PARTY.md`.

### Tests

Unit tests sit in the node's file. Graph tests in `crates/drip/tests/`
build a `Dag`, bind inputs with `node::source` and evaluate: device kernels
in `algorithms.rs` (resampling against an f64 reference in
`algorithms/resample.rs`), segments in `fusion.rs`, the store in
`store.rs`, foreign jobs and the disk tier in `foreign.rs`, regions, scale
and tiled export in `regions.rs`, reductions in `reduce.rs`, stencils in
`stencil.rs` and meta nodes in `meta.rs`. A test that counts the work of
one evaluation uses a new `Evaluator`, and a test with a disk tier gives it
a temporary directory. Tests run on the CPU runtime with
`--features drip/cpu`, and the `selected_backend` tests run on any runtime
with `--ignored`.

## GUI binding

Every listed kind appears in the editor with ports, help and schema
controls from its `Kind`, so most nodes need no GUI code. Node-specific
presentation is an `impl GuiNode` on the node's generated `Definition`,
listed in `BINDINGS` in `drip-gui/src/registry.rs`: `controls` replaces the
schema controls, `PREPARE` turns what a preview hands the node into a
drawable view, and `ACTION` adds a frontend action whose result may become
new parameters. Preparation receives the evaluator's runtime, so views read
device samples and call drip, and drip-gui launches no kernel of its own.
Preview and the scopes are declared in drip-gui with `#[drip::node]`
because what they prepare is GUI-only. The GUI worker
(`drip-gui/src/worker.rs`) runs previews, full-detail crops and actions on
one queue with one evaluator per project. The
[drip-gui README](../crates/drip-gui/README.md) has a worked example.
