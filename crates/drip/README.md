# Drip library

The headless half of Drip: the node graph, image processing, colour
management and persistence. Anything a batch run on a server would never
touch, such as preparing a vectorscope for display, belongs in `drip-gui`.

For the design, read the crate documentation (`cargo doc -p drip --open`)
and [ARCHITECTURE](../../agent-docs/ARCHITECTURE.md).

## Files

`src/node/` owns everything node-specific. Each node has a file or
directory with its parameters, contract, computation and private kernels.
Shared image-processing code sits beside the nodes: payloads (`data.rs`),
colorimetry (`color.rs`, `camera.rs`, `profile.rs`), resampling filters
(`filter.rs`) and `shared_kernel.rs`, which holds reusable operations such as
matrix multiplication and Bayer reduction, never unrelated node kernels.

Modules outside `node/` are infrastructure and name no concrete node or
payload, except `registry.rs`, which lists the kinds by hand.
[ARCHITECTURE](../../agent-docs/ARCHITECTURE.md#modules) states which of
them a node file may name.

| File | Owns |
|---|---|
| `definition.rs` | `Kind`, `Tag`, the `Node` and `Typed` traits, actions, assets, and the `Identity` and `Durability` the store keys and evicts by |
| `shape.rs` | `Shape`: how evaluation runs a kind, and the disk `Codec`s |
| `body.rs` | the bodies, filters and warps the segment compiler fuses, `Pixel`, and 3×3 `matrix` math |
| `lattice.rs` | `Lattice` and `Rect`: where samples sit on the full-detail plane |
| `plan.rs` | the segment compiler: composition rules, input windows, the fused kernel, reduction passes |
| `eval/` | `Evaluator` and its rules (`mod.rs`), one call's scheduling (`run.rs`), meta sub-graphs joining the run (`meta.rs`) and `TargetValues` (`values.rs`) |
| `store.rs` | content keys, eviction, foreign jobs, meta expansions and the `Disk` tier |
| `expand.rs` | the `SubGraph` a meta node builds |
| `image.rs` | `Image`, `Raster` and residency |
| `source.rs` | bound sources, public as `node::source` |
| `ports.rs` | port views and requirements |
| `payload.rs` | payload traits, device layouts and `Persist` streams |
| `graph.rs` | `Dag` and transactional edits checked on ports |
| `param.rs` | parameter schemas, encoding and validation |
| `project.rs` | `.drip` persistence, loaded through a `Registry` |
| `registry.rs` | `NODES`, `TAGS` and `Registry` |
| `runtime.rs` | the compute client, budgets and `DRIP_BACKEND` selection |

File organization never justifies splitting one algorithm into more nodes.

Keep one concrete Rust type per payload. Implement `DeviceLayout` only for
payloads a kernel reads, keep structured data on the host, and flatten the
numbers a kernel needs into `KernelContext::constants`. Implement `Persist`
for payloads a `foreign = disk` node outputs.

## Build and test

```sh
cargo build -p drip
DRIP_BACKEND=cpu cargo test -p drip --features cpu
cargo test -p drip --doc --features cpu
```

Cargo features include compute runtimes, and `DRIP_BACKEND` selects one at
startup. Unset, it selects `metal-native` on macOS or `vulkan` on Linux when
that feature is enabled, and `wgpu` otherwise. A selection the build does
not include fails instead of falling back. The choice does not change the
GUI renderer.

| `DRIP_BACKEND` | `drip` feature | Execution |
|---|---|---|
| `wgpu` or `wgsl` | `wgpu` (default) | WGSL through wgpu: D3D12 on Windows, wgpu's choice elsewhere |
| `dx12` | `wgpu` | the same D3D12 path, named explicitly |
| `vulkan` | `vulkan` | SPIR-V through wgpu on Vulkan |
| `metal` | `metal` | MSL through wgpu on macOS |
| `metal-native` | `metal-native` | CubeCL's own Metal runtime |
| `cuda` | `cuda` | NVIDIA CUDA |
| `hip` | `hip` | AMD HIP/ROCm |
| `cpu` | `cpu` | LLVM CPU JIT, for development and CI only |

Features are additive: `vulkan` and `metal` also include wgpu without
changing what `wgsl` means. `--no-default-features` removes wgpu. `cargo
xtask dist` enables the native choice of each platform and never ships the
CPU runtime.

To run the device tests on another runtime, build it in and name it:

```sh
DRIP_BACKEND=vulkan cargo test -p drip --features vulkan selected_backend -- --ignored
```

## Adding a node

A node is a function with `#[node]` in `src/node/<name>.rs`. These examples
are compiled as doc tests, so they use `drip::` paths. Inside the crate,
write `crate::`.

The worked example doubles every RGB sample in a kernel:

```rust,no_run
use drip::{
    Result,
    eval::Evaluator,
    graph::Dag,
    node::{self, data::*},
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Double every RGB sample (+1 EV). The output keeps the input coordinates.
#[drip::node(id = "double", name = "Double", tags = [tag::LOOK], contract = contract)]
pub fn compute(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Rgb>,
    mut output: Write<'_, Rgb>,
) -> Result<()> {
    let input = image.data.host()?;
    for (out, px) in output.host().iter_mut().zip(input.iter()) {
        *out = px.map(|v| v * 2.0);
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut dag = Dag::new();
    let desc = RgbDesc::new(Extent { width: 2, height: 1 }, Coordinates::Space(REC2020_D65));
    let source = node::source::<Rgb>(&mut dag, desc, vec![[0.1, -0.2, 2.0]; 2])?;
    let double = compute::add(&mut dag, ())?;
    dag.connect(source, double.image)?;
    let result = Evaluator::new(RuntimeContext::from_env()?)
        .evaluate::<Rgb>(&dag, &Default::default(), double.output)?;
    assert_eq!(&**result.data.host()?, &[[0.2, -0.4, 4.0]; 2]);
    Ok(())
}
```

1. Declare the module in `src/node/mod.rs`.
2. List `&double::compute::KIND` in `registry::NODES` at its pipeline
   position. A kind missing there cannot be added or loaded.
3. Test it through a graph, as `main` does. Graph tests live in `tests/`.

`#[node]` generates the rest: the `Kind` with help from the Rustdoc, `add`,
and typed `Ports` and `Inputs`. The contract runs at evaluation and
describes the outputs, so reject inputs the node cannot handle there. A
kernel writes `output.host()` or `output.device(ctx)` and reads
`image.data.host()?` or `image.data.device(ctx)?`, and the raster transfers
what the next node asks for. `reduce/rgb.rs` is a device kernel.

The ID is the persistence key, so keep it once projects use it. Bump
`version = N` when the node computes something else from the same
parameters and inputs, since the store keys outputs by it. Parameters are a
struct deriving `Parameters` in place of `()`, as in `exposure.rs`. Credit
adapted algorithms beside the code and in `THIRD_PARTY.md`.

### Map

A per-pixel operation on `Rgb` is better declared as a map. Its host
function returns the constants of a `#[cube]` body, and evaluation runs a
chain of maps as one kernel and stores only its result. `exposure.rs` is the
smallest real map.

```rust
use drip::{Result, node::data::*, ports::*, registry::tag, runtime::GlobalContext};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Double every RGB sample (+1 EV). The output keeps the input coordinates.
#[drip::node(id = "double", name = "Double", tags = [tag::LOOK], contract = contract, map = kernel::double)]
pub fn compute(_: &(), _image: Point<'_, Rgb>, _output: Point<'_, Rgb>) -> Result<[f32; 1]> {
    Ok([2.0])
}

mod kernel {
    use cubecl::prelude::*;
    use drip::body::Pixel;
    #[cube]
    pub fn double(px: Pixel, k: &[f32]) -> Pixel {
        Pixel { r: px.r * k[0], g: px.g * k[0], b: px.b * k[0] }
    }
}
# fn main() {}
```

The first `Point` is the mapped input and the second the output. Other
inputs are `Read` and arrive materialized, as `camera_to_rgb.rs` reads its
metadata. A warp, `#[node(warp)]`, has the same ports and returns a `Warp`
of projective maps from the output plane to the input plane, as in
`orientation.rs` and `perspective.rs`. `resample.rs` is the one resample.

### Foreign node

Host work over whole images, or work outside CubeCL such as a decoder, is a
foreign node. It runs on a thread of its own at full detail, once per key,
and describes each output as it publishes it. `hot_pixels.rs` is the
smallest real one.

```rust
use drip::{Result, node::data::*, ports::*, registry::tag};

/// Double every RGB sample (+1 EV) on the host. The output keeps the input
/// coordinates.
#[drip::node(id = "double", name = "Double", tags = [tag::LOOK], foreign = memory)]
pub fn compute(_: &(), image: Read<'_, Rgb>, output: Publish<'_, Rgb>) -> Result<()> {
    let doubled: Vec<_> = image.data.host()?.iter().map(|px| px.map(|v| v * 2.0)).collect();
    output.set(image.desc.clone(), doubled.into())
}
# fn main() {}
```

`foreign = disk` also keeps the outputs on disk, so another evaluator or
process reads them instead of running the node, as `raw.rs` does for RAW
decode. Declare it when running the node costs more than reading its
outputs. A foreign node's key includes the path, length and modification
time of the files its `Path` parameters name.

### Reduction

A measurement over every pixel is a reduce or bins node. Its body
accumulates inside the input's segment, so measuring a chain of maps stores
no image. This one counts samples by tenths of each channel. The scopes in
`drip-gui/src/node_ui/scopes/` are real bins nodes.

```rust
use drip::{Result, body::BinsBody, node::data::*, ports::*, runtime::GlobalContext};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(CountsDesc,)> {
    Ok((CountsDesc { bins: 10, channels: 3, coordinates: image.coordinates.clone() },))
}

/// Count RGB samples in `[0, 1)` per tenth of each channel.
#[drip::node(id = "tenths", name = "Tenths", contract = contract, bins = TENTHS)]
pub fn compute(_: &(), _image: Point<'_, Rgb>, _counts: Point<'_, Counts>) -> Result<[f32; 0]> {
    Ok([])
}

static TENTHS: BinsBody =
    BinsBody { name: "tenths", constants: 0, bins: 10, channels: 3, bin: kernel::tenths::expand };

mod kernel {
    use cubecl::prelude::*;
    use drip::body::{Bin, Pixel, Position};
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
# fn main() {}
```

`reduce = BODY` names a `ReduceBody`, which folds pixels into an
accumulator of f32. A frontend that targets the node binds its outputs with
`TargetValues::bind_outputs`. To measure without a node, call `Image::fold`
or `Image::count`, as `exposure::meter` does for auto exposure.

### Stencil

An operation on a neighbourhood of samples is a stencil. Its host function
returns how far the body reads, in full-detail pixels, and the body's
constants. Evaluation converts the reach to samples of the lattice the
stencil reads, so the node runs at every preview scale, and a crop of its
output computes only the crop and the input under its reach. `sharpen.rs`
is the real one.

```rust
use drip::{
    Result,
    body::{Border, Stencil, StencilBody},
    node::data::*,
    ports::*,
    runtime::GlobalContext,
};

fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}

/// Average each RGB sample with its neighbours within 2 full-detail pixels.
#[drip::node(id = "soften", name = "Soften", contract = contract, stencil = MEAN)]
pub fn compute(_: &(), _image: Point<'_, Rgb>, _output: Point<'_, Rgb>) -> Result<Stencil<0>> {
    Ok(Stencil { radius: 2.0, constants: [] })
}

static MEAN: StencilBody =
    StencilBody { name: "soften::mean", constants: 0, border: Border::Repeat, expand: kernel::mean::expand };

mod kernel {
    use cubecl::prelude::*;
    use drip::body::{Pixel, Window};
    #[cube]
    pub fn mean(w: &Window, _k: &[f32]) -> Pixel {
        let mut r = 0.0f32;
        let mut g = 0.0f32;
        let mut b = 0.0f32;
        let mut n = 0.0f32;
        for dy in -w.radius_y..w.radius_y + 1 {
            for dx in -w.radius_x..w.radius_x + 1 {
                let px = w.at(dx, dy);
                r += px.r;
                g += px.g;
                b += px.b;
                n += 1.0f32;
            }
        }
        Pixel { r: r / n, g: g / n, b: b / n }
    }
}
# fn main() {}
```

A body that weighs taps by distance scales its lengths by `w.rate_x` and
`w.rate_y`, samples per full-detail pixel, as `sharpen.rs` does.

### Meta node

An operation built from other nodes is a meta node. Its function adds nodes
to a sub-graph and wires them to its ports, and evaluation runs those nodes
in its place. `pyramid.rs` is the real one and sizes its last Resample from
`image.desc`.

```rust
use drip::{
    Result,
    expand::SubGraph,
    node::{data::*, filter::Filter, resample, sharpen},
    ports::*,
};

/// Soften RGB with a Gaussian of 1 full-detail pixel, then halve it.
#[drip::node(id = "soft-half", name = "Soft half", meta)]
pub fn expand(_: &(), graph: &mut SubGraph, image: Inlet<'_, Rgb>, output: Outlet<Rgb>) -> Result<()> {
    let soften = sharpen::add(&mut graph.dag, sharpen::Settings { radius: 1.0, amount: -1.0 })?;
    let size = resample::Size::Factor(resample::Factor { factor: 0.5 });
    let half = resample::add(&mut graph.dag, resample::Settings { filter: Filter::Box, size })?;
    graph.input(image, soften.image)?;
    graph.dag.connect(soften.output, half.image)?;
    graph.output(half.output, output)
}
# fn main() {}
```

## Labels, tags and parameters

IDs name the operation in kebab case (`exposure`, `rcd-demosaic`). Display
labels use sentence case and keep technical spellings such as RAW, RGB, ICC
and Rec.2020. Neither decides compatibility. Tags say what the node is for:
at most one ACES stage tag, then any others that help finding it
(`registry::tag`).

`Parameters` and `Choice` generate defaults, schemas and Serde adapters
from typed declarations (see their Rustdoc). Keep settings that only one
choice uses, such as a profile path or a compression level, inside that
choice's variant. File extensions are file-format facts in the schema, and
the GUI derives its dialogs from them.

## Store directory

An evaluator given `Disk::platform()` through `Evaluator::with_disk` keeps
the outputs of `foreign = disk` nodes, such as decoded RAWs, in `drip/store`
under the platform cache
directory (`$XDG_CACHE_HOME/drip/store` or `~/.cache/drip/store` on Linux),
within 16 GiB. Each output is one file named by its content key. You can
delete the directory, or any file in it, at any time.
