# Drip library

## Organization

- `src/node/` owns node-specific behavior. Each node has a file or directory
  containing its parameters, contracts, computation and private helpers/kernels.
  RCD kernels belong in `node/rcd/`; sigmoid math belongs in `node/sigmoid.rs`.
- Shared image-processing code also lives under `node/`: payload implementations,
  color management, export encoding and pipeline templates. `shared_kernel.rs`
  contains reusable operations such as matrix multiplication and Bayer reduction,
  never a collection of unrelated node kernels.
- Outside `node/`, keep shared DAG/evaluation infrastructure: definitions and
  discovery, ports and payload interfaces, graph checks, scheduling, runtime
  access, parameter schemas, node file assets and generic project persistence. These
  modules must not depend on concrete image-processing nodes or payloads.

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

## Node metadata and parameters

Declare operation names, categories, help and port contracts beside node functions.
Registered IDs use `category.operation`; display labels use sentence case and
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
distance describes the lens, not this exposure's focus. TIFF writes the selected
EXIF subset and ICC profile.
