# Organization

- `src/node/` owns node-specific behavior. Each node has a file or directory
  containing its parameters, contracts, computation and private helpers/kernels.
  RCD kernels belong in `node/rcd/`; sigmoid math belongs in `node/sigmoid.rs`.
- Shared image-processing code also lives under `node/`: payload implementations,
  color management, export encoding and pipeline templates. `shared_kernel.rs`
  contains reusable operations such as matrix multiplication and Bayer reduction,
  never a collection of unrelated node kernels.
- Outside `node/`, keep shared DAG/evaluation infrastructure: definitions and
  discovery, ports and payload interfaces, graph checks, scheduling, runtime
  access, parameter schemas, resources and generic project persistence. These
  modules must not depend on concrete image-processing nodes or payloads.

The same ownership rule applies to `drip-gui`: node-specific declarations,
contracts, controls, preparation and presentation belong under `src/node_ui/`.
Each GUI node has its own module; shared helpers stay alongside them.
Windowing, the graph editor, worker scheduling and shared rendering machinery
remain outside. File organization does not justify splitting one algorithm
into more DAG nodes or changing GUI interaction behavior.

Payloads keep concrete Rust data types and separate interpretation descriptions.
Device allocation and transport are shared; a payload may supply a small local
packing adapter for nested allocations. Do not duplicate CPU/GPU schemas or
flatten unrelated ports merely to make their storage uniform.

# Compute backends

Cargo features include compute runtimes; `DRIP_BACKEND` selects one at startup.
The default build and unset environment variable both select `wgpu`. CPU/LLVM
is opt-in for development and is not part of the desktop distribution plan.
These choices do not change the GUI renderer.

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
