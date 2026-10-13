# Drip library

## Organization

- `src/node/` owns node-specific behavior. Each node has a file or directory
  containing its parameters, contracts, computation and private helpers/kernels.
  RCD kernels belong in `node/rcd/`; sigmoid math belongs in `node/sigmoid.rs`.
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
Each GUI node has its own module; shared helpers stay alongside them.
Windowing, the graph editor, worker scheduling and shared rendering machinery
remain outside. File organization does not justify splitting one algorithm
into more DAG nodes or changing GUI interaction behavior.

Payloads keep concrete Rust data types and a host description that contracts
read. A payload implements `DeviceLayout` only when a kernel reads it on the
device; `Device<P>` ports require it, and structured data stays on the host.
Nodes flatten the numbers a kernel needs into `KernelContext::constants`.
Interpretations are typed values in descriptions (camera responses or numeric
RGB spaces, compared exactly). Ports declare requirements as type arguments,
`Read<'_, Device<Rgb>, Rec2020>`; the checker, help and editor read the same
declaration. Do not flatten unrelated ports merely to make storage uniform.

## Files

The non-trivial files outside `node/`:

- `definition.rs`: what `#[node]` generates against: `Kind`, `Tag`, the
  `Node`/`Typed` traits, `Action`, `Assets`.
- `registry.rs`: the hand-maintained `NODES`, `TAGS` and `Registry`.
- `graph.rs`: `Dag` and transactional edits, checked on ports only.
- `eval.rs`: `Evaluator`, demand-driven evaluation of the requested subgraph;
  contracts run here.
- `ports.rs`: port views (`Read`/`Write`/`Load`), `Cpu`/`Device` residency and
  interpretation requirements.
- `payload.rs`: payload families and their host/device storage.
- `param.rs`: parameter schemas, encoding and validation.
- `project.rs`: `.drip` persistence, loaded through a `Registry`.
- `runtime.rs`: one compute client per evaluator; `DRIP_BACKEND` selection.

## Registries

Everything a frontend discovers is listed by hand in a registry module, not
collected at link time. A kind missing from a list cannot be added or loaded;
a new node is one more line.

- `drip::registry::NODES`: processing node kinds, in pipeline order.
- `drip::registry::TAGS`: every tag a node may carry, with a one-sentence
  description; frontends add none. A node takes at most one stage tag, after
  ACES: `input-transform`, `look` or `output-transform`. File readers and
  writers carry `io`.
- `drip::registry::Registry`: composes lists, rejects duplicate IDs and
  admits persisted parameters. `Project::from_json` loads through the registry
  it is given, so a frontend states which kinds it knows. A headless frontend
  passes `NODES`.

`drip-gui` keeps its own lists in `src/registry.rs`; see its
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
    image: Read<'_, Device<Rgb>>,
    output: Write<'_, Device<Rgb>>,
) -> Result<()> {
    let (count, dim) = dispatch_dims(image.desc.extent.pixels() * 3);
    let (input, output) = (image.data.argument(), output.data.argument());
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
       .evaluate::<Cpu<Rgb>>(&dag, &Default::default(), double.output)?;
   assert_eq!(&**result.data, &[[0.2, -0.4, 4.0]; 2]);
   ```

`#[node]` generates the rest: the `Kind` (ID, help from the Rustdoc, ports,
schema), `add`, typed `Ports` and `Inputs`. The contract runs at evaluation
and states what the output describes; reject inputs it cannot handle there.
The ID is the persistence key, so keep it once projects use it. Tags say what
the node is for: at most one stage tag, then any others that help finding it.
Parameters are a struct deriving `Parameters` in place of `()`; see
`exposure.rs`. Credit adapted algorithms beside the code and in
`THIRD_PARTY.md`. Every frontend now offers the node with schema controls; for
GUI-specific presentation see the [drip-gui README](../drip-gui/README.md).

## Node metadata and parameters

Declare operation names, tags, help and port contracts beside node functions.
IDs name the operation in kebab case (`exposure`, `rcd-demosaic`) and are the
persistence keys; display labels use sentence case and
retain technical spellings such as RAW, RGB, ICC, sRGB and Rec.2020. Labels do
not determine compatibility or persistence keys.

`Parameters` generates `Default`, field schemas and Rustdoc help from one typed
declaration per field: `#[param(default = 1.5, range = 0.5..=4.0)]`, a choice
default, `open`/`save` file extensions, or `flatten`. `Choice` generates Serde
adapters, schemas and `Default` from its `#[default]` variant, for unit variants
or variants containing a parameter struct. Unit variants serialize as strings; variants with settings use Serde's
externally tagged object form. The GUI renders only the active variant. Keep
profile-file paths, proof settings and compression levels in the variants that
consume them, rather than independent inactive fields.

File extensions are file-format facts in the schema; the GUI derives its
dialogs from them. Scope annotations belong to `drip-gui`. The library owns
processing values and their interpretations. Sources use the same `#[node]`
declaration with `Load` ports, `fn(&Settings, Load<'_, P>...)`: they load once,
when first evaluated, and the node keeps the shared values or the failure.
Files are assumed not to change while a node names them.

## Compute backends

Cargo features include compute runtimes; `DRIP_BACKEND` selects one at startup.
An unset environment variable selects `metal-native` on macOS if that feature
is enabled, or `vulkan` on Linux if enabled; otherwise it selects `wgpu`. An
ordinary default-feature build uses wgpu. `cargo xtask dist` enables the native
platform choice, using WGSL/D3D12 on Windows. CPU/LLVM is opt-in for development
and is not shipped by xtask. These choices do not change the GUI renderer.

| `DRIP_BACKEND` | Required `drip` feature | Execution |
| --- | --- | --- |
| `wgpu` or `wgsl` | `wgpu` (default) | WGSL through wgpu; D3D12 on Windows, automatic API elsewhere |
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
required. For library-only builds, `--no-default-features` removes wgpu; select
an included backend explicitly if calling `RuntimeContext::from_env()`.

Run the shared algorithm checks on any configured runtime with
`DRIP_BACKEND=vulkan cargo test -p drip --features vulkan selected_backend -- --ignored`.

## RAW metadata

RAW exposes `mosaic: Bayer` and `metadata: CaptureMetadata`. The metadata is
what LibRaw reported: capture settings, lens identifiers, optional GPS, sensor
geometry and native WB/black/white/color calibration. Unknown values retain
LibRaw's sentinels. Vendor-specific maker notes and correction profiles are not
included. It is a host-only value whose description is the shared value itself.

Consumers derive what they apply (`node::camera`). `Bayer site gains` derives
as-shot gains and clipping levels and records the balance in the Bayer response.
Camera conversion derives its matrix from `xyz_to_cam` and that balance,
replacing it with D65 reference gains; CAT16 maps the as-shot camera neutral
through the same matrix for its initial source white. As-shot conditioning
remains in place through highlights and demosaic. The minimum lens focus
distance describes the lens, not this exposure's focus. TIFF and JPEG write the
selected EXIF subset and ICC profile. JPEG converts to 8-bit output through the
same profile settings; quality and chroma subsampling control its lossy encoding.
