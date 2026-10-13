# Drip library

## Organization

- `src/node/` owns node-specific behavior. Each node has a file or directory
  containing its parameters, contracts, computation and private helpers/kernels.
  RCD kernels belong in `node/rcd/`, sigmoid math in `node/sigmoid.rs`.
- Shared image-processing code also lives under `node/`: payload implementations,
  color management and export encoding. `shared_kernel.rs`
  contains reusable operations such as matrix multiplication and Bayer reduction,
  never a collection of unrelated node kernels.
- Outside `node/`, keep shared DAG/evaluation infrastructure: definitions and
  discovery, ports and payload interfaces, graph checks, scheduling, runtime
  access, parameter schemas, node file assets and generic project persistence. These
  modules must not depend on concrete image-processing nodes or payloads.
- `src/registry.rs` is the one exception: the composition point that lists
  every node kind by hand. See [Registries](#registries).

The same ownership rule applies to `drip-gui`: node-specific declarations,
contracts, controls, preparation and presentation belong under `src/node_ui/`.
Each GUI node has its own module, and code shared by GUI nodes stays beside
them.
Windowing, the graph editor, worker scheduling and shared rendering machinery
remain outside. File organization does not justify splitting one algorithm
into more DAG nodes or changing GUI interaction behavior.

[ARCHITECTURE](../../agent-docs/ARCHITECTURE.md) describes ports, payloads,
residency, shapes and evaluation. Keep a concrete Rust type per payload.
Implement `DeviceLayout` only for payloads a kernel reads, keep structured
data on the host, and flatten the numbers a kernel needs into
`KernelContext::constants`. Implement `Persist` for payloads a
`foreign = disk` node outputs. Do not flatten unrelated ports merely to make
storage uniform.

## Files

The non-trivial files outside `node/`:

- `definition.rs`: what `#[node]` generates against: `Kind`, `Tag`, the
  `Node`/`Typed` traits, `Action`, `Assets`, and the `Identity` and
  `Durability` the store keys and evicts by.
- `shape.rs`: how evaluation runs a kind (`Shape`: kernel, bound source,
  foreign, map, warp, resample, reduce or bins, and whether it reads the
  scale), where a foreign node's outputs are kept (`Cache`, `Codec`), and
  what the segment compiler composes: `MapBody` and the `Pixel` it maps,
  `Warp`, `FilterBody` and the `BOX` filter that reduces to the requested
  scale, `ReduceBody` and `BinsBody` with the `Position` and `Bin` a bin
  body sees and returns, and the `Lattice` of a `MapPayload` with the window
  a raster holds, in `Rect`s.
- `registry.rs`: the hand-maintained `NODES`, `TAGS` and `Registry`.
- `graph.rs`: `Dag` and transactional edits, checked on ports only.
- `eval.rs`: `Evaluator`, demand-driven evaluation of the requested subgraph;
  contracts run here, nodes whose outputs the store holds are skipped, and
  results are materialized for their consumers. It decides the scale each
  node runs at, reduces full-detail images to the requested scale and cuts a
  `Target`'s inputs to its region. `Policy` says whether a run waits for a
  foreign job or reports it pending, and `InputValues` hands frontends and
  actions the inputs, whole or in bands, or a reduction's outputs.
- `image.rs`: `Image`, the value on an edge, a raster or the deferred output of
  a map, warp or resample, materialized on first need, whole or by window
  (`Image::crop`), or reduced to host values without storing it
  (`Image::fold`, `Image::count`), `Raster`, samples with lazy host and
  device residency and counted transfers, and `Plane`, what evaluation does
  to an image on a plane without its type.
- `plan.rs`: the segment compiler: a `Plan` of one resampling stage and map
  bodies, the rules for composing steps into it, the input window a window of
  its output reads, the fused kernel that runs it on a window, and the
  passes that fold or count its pixels.
- `store.rs`: the evaluator's memo of materialized values across demands:
  content keys, eviction by durability and recency within per-side byte
  budgets, foreign jobs, and the `Disk` tier.
- `ports.rs`: port views (`Read`, `Write`, `Forward`, `Publish` for foreign
  nodes, and `Point` for maps, warps and resamples) and interpretation
  requirements.
- `payload.rs`: payload families, their host types, device byte layouts and
  `Persist` streams.
- `param.rs`: parameter schemas, encoding and validation.
- `project.rs`: `.drip` persistence, loaded through a `Registry`.
- `runtime.rs`: one compute client per evaluator and `DRIP_BACKEND` selection.

## Registries

Everything a frontend discovers is listed by hand in a registry module, not
collected at link time. A kind missing from a list cannot be added or loaded;
a new node is one more line.

- `drip::registry::NODES`: processing node kinds, in pipeline order.
- `drip::registry::TAGS`: every tag a node may carry, with a one-sentence
  description and the durability of its nodes' outputs in the store. Frontends add none. A node takes at most one stage tag, after
  ACES: `input-transform`, `look` or `output-transform`. File readers and
  writers carry `io`.
- `drip::registry::Registry`: composes lists, rejects duplicate IDs and
  admits persisted parameters. `Project::from_json` loads through the registry
  it is given, so a frontend states which kinds it knows. A headless frontend
  passes `NODES`.

`drip-gui` keeps its own lists in `src/registry.rs`, described in its
[README](../drip-gui/README.md).

## Adding a node

A node that doubles every RGB sample, as `src/node/double.rs`:

```rust
use crate::{
    Result,
    node::data::*,
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Double every RGB sample (+1 EV). Output keeps the input coordinates.
#[crate::node(id = "double", name = "Double", tags = [tag::LOOK], contract = contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Rgb>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let (count, dim) = dispatch_dims(image.desc.extent.pixels() * 3);
    let (input, output) = (image.data.device(ctx)?.argument(), output.device(ctx)?.argument());
    kernel::double::launch(ctx.client()?, count, dim, input, output);
    Ok(())
}

mod kernel {
    use cubecl::prelude::*;
    #[cube(launch)]
    pub fn double(input: &[f32], output: &mut [f32]) {
        let i = ABSOLUTE_POS;
        if i < input.len() {
            output[i] = input[i] * 2.0;
        }
    }
}
```

A per-pixel operation is better declared as a map. Its host function returns
the constants of a `#[cube]` body that maps one pixel. Evaluation runs a chain
of maps, such as the template's exposure and sigmoid, as one kernel and stores
only its result:

```rust
use crate::{Result, node::data::*, ports::*, registry::tag, runtime::GlobalContext};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Double every RGB sample (+1 EV). Output keeps the input coordinates.
#[crate::node(id = "double", name = "Double", tags = [tag::LOOK], contract = contract, map = kernel::double)]
pub fn compute(_: &(), _image: Point<'_, Rgb>, _output: Point<'_, Rgb>) -> Result<[f32; 1]> {
    Ok([2.0])
}

mod kernel {
    use crate::shape::Pixel;
    use cubecl::prelude::*;
    #[cube]
    pub fn double(px: Pixel, k: &[f32]) -> Pixel {
        Pixel { r: px.r * k[0], g: px.g * k[0], b: px.b * k[0] }
    }
}
```

The first `Point` is the mapped input and the second the output. Other inputs
are `Read` and arrive materialized, as Camera RGB to color RGB reads its
`metadata`. A map has no `KernelContext` because it launches nothing.
Geometry is a warp declared the same way, `#[node(warp, ...)]`, whose host
function returns a `Warp` of projective maps from the output plane to the
input plane, as in `node/orientation.rs` and `node/perspective.rs`.

Host work over whole images, or work outside CubeCL such as a decoder, is a
foreign node. It runs on a thread of its own at full detail, once per key,
and has no contract because it describes each output as it publishes it:

```rust
use crate::{Result, node::data::*, ports::*, registry::tag};

/// Double every RGB sample (+1 EV) on the host. Output keeps the input
/// coordinates.
#[crate::node(id = "double", name = "Double", tags = [tag::LOOK], foreign = memory)]
pub fn compute(_: &(), image: Read<'_, Rgb>, output: Publish<'_, Rgb>) -> Result<()> {
    let doubled: Vec<_> = image.data.host()?.iter().map(|px| px.map(|v| v * 2.0)).collect();
    output.set(image.desc.clone(), doubled.into())
}
```

`foreign = disk` also keeps the outputs on disk, so another evaluator or
process reads them instead of running the node, as `node/raw.rs` does for
RAW decode. A foreign node's key includes the path, length and modification
time of the input files its `Path` parameters name.

A measurement over every pixel is a reduce or bins node. Its body counts or
accumulates inside the input's segment, so measuring a chain of maps stores
no image. A node that counts samples by tenths of each channel:

```rust
use crate::{Result, node::data::*, ports::*, runtime::GlobalContext, shape::BinsBody};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(CountsDesc,)> {
    Ok((CountsDesc { bins: 10, channels: 3, coordinates: image.coordinates.clone() },))
}

/// Count RGB samples in `[0, 1)` per tenth of each channel.
#[crate::node(id = "tenths", name = "Tenths", contract = contract, bins = TENTHS)]
pub fn compute(_: &(), _image: Point<'_, Rgb>, _counts: Point<'_, Counts>) -> Result<[f32; 0]> {
    Ok([])
}

static TENTHS: BinsBody =
    BinsBody { name: "tenths", constants: 0, bins: 10, channels: 3, bin: kernel::tenths::expand };

mod kernel {
    use crate::shape::{Bin, Pixel, Position};
    use cubecl::prelude::*;
    #[cube]
    fn tenth(v: f32) -> u32 {
        let mut bin = 10u32; // counts nothing
        if v >= 0.0f32 && v < 1.0f32 {
            bin = u32::cast_from(v * 10.0f32);
        }
        bin
    }
    #[cube]
    pub fn tenths(px: Pixel, _at: Position, _k: &[f32]) -> Bin {
        Bin { c0: tenth(px.r), c1: tenth(px.g), c2: tenth(px.b) }
    }
}
```

`reduce = BODY` names a `ReduceBody` instead, which folds pixels into an
accumulator of f32. A frontend that targets the node receives its outputs
and binds them with `InputValues::bind_outputs`. One that wants a
measurement without a node calls `Image::fold` or `Image::count`, as
`exposure::meter` does.

1. Declare the module: `pub mod double;` in `src/node/mod.rs`.
2. List `&double::compute::KIND` in `registry::NODES`, at its pipeline
   position. Without this the node cannot be added or loaded.
3. Test it through a graph:

   ```rust
   let mut dag = Dag::new();
   let source = node::source::<Rgb>(&mut dag, desc, vec![[0.1, -0.2, 2.0]; 2])?;
   let double = node::double::compute::add(&mut dag, ())?;
   dag.connect(source, double.image)?;
   let result = Evaluator::new(RuntimeContext::from_env()?)
       .evaluate::<Rgb>(&dag, &Default::default(), double.output)?;
   assert_eq!(&**result.data.host()?, &[[0.2, -0.4, 4.0]; 2]);
   ```

`#[node]` generates the rest: the `Kind` (ID, help from the Rustdoc, ports,
schema, shape), `add`, typed `Ports` and `Inputs`. The contract runs at
evaluation and states what the output describes. Reject inputs it cannot
handle there. A host node writes `output.host()` instead of `output.device(ctx)`
and reads `image.data.host()?`. The raster transfers what the next node asks
for. An output declared `Forward<'_, P>` instead of `Write` is set from samples
the node already holds, `output.set(image.data)`, so a node that only changes
the description copies nothing.
A kernel receives whole inputs and scale 1. At a preview scale its RGB
inputs arrive reduced. Declare
`scaled` (`#[node(..., scaled)]`) only for a kernel that reaches the
requested scale itself, cheaper or better than evaluation's box reduction,
as the demosaics do with `reduce::for_demosaic`. Its contract and function
then receive `GlobalContext::scale`, its output is taken as produced at that
scale, and its outputs are keyed by it. Kernels declare no `halo` yet: they
run whole, and a region bounds only the fused segments after them.
The ID is the persistence key, so keep it once projects use it. The
evaluator stores outputs under keys that include `version`, 1 by default:
add `version = 2` to `#[node]` when a change alters what the node computes
from the same parameters and inputs. Tags say what
the node is for: at most one stage tag, then any others that help finding it.
Parameters are a struct deriving `Parameters` in place of `()`, as in
`exposure.rs`. Credit adapted algorithms beside the code and in
`THIRD_PARTY.md`. Every frontend offers the node with schema controls. For
GUI-specific presentation, see the [drip-gui README](../drip-gui/README.md).

## Node metadata and parameters

Declare operation names, tags, help and port contracts beside node functions.
IDs name the operation in kebab case (`exposure`, `rcd-demosaic`) and are the
persistence keys. Display labels use sentence case and
retain technical spellings such as RAW, RGB, ICC, sRGB and Rec.2020. Labels do
not determine compatibility or persistence keys.

`Parameters` generates `Default`, field schemas and Rustdoc help from one typed
declaration per field: `#[param(default = 1.5, range = 0.5..=4.0)]`, a choice
default, `open`/`save` file extensions, or `flatten`. `Choice` generates Serde
adapters, schemas and `Default` from its `#[default]` variant, for unit variants
or variants containing a parameter struct. Unit variants serialize as strings,
and variants with settings use Serde's externally tagged object form. The GUI renders only the active variant. Keep
profile-file paths, proof settings and compression levels in the variants that
consume them, rather than independent inactive fields.

File extensions are file-format facts in the schema, and the GUI derives its
dialogs from them. Scope annotations belong to `drip-gui`. The library owns
processing values and their interpretations.

## Store directory

An evaluator keeps values in memory. `Evaluator::with_disk(Disk)` adds a disk
tier for `foreign = disk` outputs, such as decoded RAWs, which outlives the
process. Frontends pass `Disk::platform()`: `drip/store` in the platform
cache directory, `$XDG_CACHE_HOME/drip/store` or `~/.cache/drip/store` on
Linux, within 16 GiB. Each output is one file named by its content key. You can
delete the directory, or any file in it, at any time.

## Compute backends

Cargo features include compute runtimes, and `DRIP_BACKEND` selects one at
startup.
An unset environment variable selects `metal-native` on macOS if that feature
is enabled, or `vulkan` on Linux if enabled, and otherwise `wgpu`. An
ordinary default-feature build uses wgpu. `cargo xtask dist` enables the native
platform choice, using WGSL/D3D12 on Windows. CPU/LLVM is opt-in for development
and is not shipped by xtask. These choices do not change the GUI renderer.

| `DRIP_BACKEND` | Required `drip` feature | Execution |
| --- | --- | --- |
| `wgpu` or `wgsl` | `wgpu` (default) | WGSL through wgpu, D3D12 on Windows, automatic API elsewhere |
| `dx12` | `wgpu` | Explicit Windows spelling of the same D3D12/WGSL path |
| `vulkan` | `vulkan` | Direct SPIR-V through wgpu on Vulkan |
| `metal` | `metal` | Direct MSL through wgpu on macOS |
| `metal-native` | `metal-native` | CubeCL's independent Metal runtime on macOS |
| `cuda` | `cuda` | NVIDIA CUDA |
| `hip` | `hip` | AMD HIP/ROCm |
| `cpu` | `cpu` | LLVM CPU JIT |

For example, build with `cargo build --release -p drip-gui --features drip/vulkan`,
then launch with `DRIP_BACKEND=vulkan target/release/drip-gui` or
`DRIP_BACKEND=wgsl target/release/drip-gui`. Metal builds similarly use
`--features drip/metal,drip/metal-native` on macOS. On Windows the default build
uses D3D12; PowerShell can explicitly select it with `$env:DRIP_BACKEND='dx12'`.

Features are additive. Enabling `vulkan` or `metal` also includes wgpu, but never
changes what `DRIP_BACKEND=wgsl` means. An unavailable selection fails instead of
switching compiler or device runtime. Drivers and hardware support are still
required. For library-only builds, `--no-default-features` removes wgpu. Select
an included backend explicitly if calling `RuntimeContext::from_env()`.

Run the shared algorithm checks on any configured runtime with
`DRIP_BACKEND=vulkan cargo test -p drip --features vulkan selected_backend -- --ignored`.

## RAW metadata

RAW decode is a `foreign = disk` node with outputs `mosaic: Bayer` and
`metadata: CaptureMetadata`. The metadata is what LibRaw reported: capture
settings, lens identifiers, optional GPS, sensor geometry and native
WB/black/white/color calibration. Unknown values retain
LibRaw's sentinels. Vendor-specific maker notes and correction profiles are not
included. It is a host-only value whose description is the shared value itself.

Consumers derive what they apply (`node::camera`). `Bayer site gains` derives
as-shot gains and clipping levels and records the balance in the Bayer response.
Camera conversion derives its matrix from `xyz_to_cam` and that balance,
replacing it with D65 reference gains. CAT16 maps the as-shot camera neutral
through the same matrix for its initial source white. As-shot conditioning
remains in place through highlights and demosaic. The minimum lens focus
distance describes the lens, not this exposure's focus. TIFF and JPEG write the
selected EXIF subset and ICC profile. JPEG converts to 8-bit output through the
same profile settings. Quality and chroma subsampling control its lossy
encoding.
