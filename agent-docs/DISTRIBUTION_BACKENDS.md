# Desktop compute backend selection

## User requirements

- **Requirement:** expose backend build options and environment selection,
  validate and commit this implementation, then research distribution choices.
- **Requirement:** do not distribute the CPU runtime for now; its LLVM JIT is
  too large. Keep it available for development.
- **Requirement:** Windows needs D3D12. Compare Linux WGSL versus direct
  SPIR-V, and macOS wgpu/MSL versus native Metal before choosing distribution
  defaults. This work does not target headless distribution.
- **Requirement (follow-up):** choose the adventurous performance candidates
  for distribution and have xtask check the platform.

## Implementation

- **Decided:** `drip` features are `wgpu` (default), `vulkan`, `metal`,
  `metal-native`, `cuda`, `hip`, and `cpu`. The crate README documents their
  matching `DRIP_BACKEND` values. `wgsl` aliases `wgpu`; `dx12` selects the
  Windows WGSL path explicitly.
- **Decided:** WGSL, SPIR-V and MSL selectors name explicit compiler types.
  Enabling a feature must not silently change an explicit compiler selection.
  `wgpu` uses D3D12 on Windows and automatic graphics API selection elsewhere.
  No automatic CPU or vendor-runtime fallback.
- **Decided:** xtask builds Linux with `drip/vulkan`, macOS with
  `drip/metal-native`, and Windows with `drip/wgpu`. Startup prefers those native
  backends when compiled in for their platform. An explicit `DRIP_BACKEND`
  overrides the default; WGSL remains available. There is no failure fallback.
- **Decided:** xtask asks rustc for the destination OS and rejects unsupported
  platforms. Omitted `--target` means the compiler's host, passed explicitly to
  Cargo so configured Cargo targets cannot change the artifact's backend choice.
- **Open:** package size and native platform validation. These choices are
  performance candidates, not measured Drip winners. GUI behavior, evaluator
  transport and kernels are unchanged.

## Session validation

- Follow-up xtask change: compiler-host resolution and all six CI target triples
  pass the platform-selection test; unsupported/invalid targets are rejected.
  Vulkan-enabled runtime default/override selection tests and all-target Clippy
  for xtask/drip pass. No distribution binaries were assembled in this session;
  macOS/Windows runtime validation remains outstanding.
- Vulkan-enabled `drip`/`drip-gui` build and all-target Clippy passed.
- Library unit tests passed. Existing algorithm, opposed-highlight reference,
  and hybrid-transfer tests passed on hardware with both `DRIP_BACKEND=vulkan`
  and `DRIP_BACKEND=wgsl`, using the same Vulkan-enabled test binaries.
- `drip --no-default-features` compiles. The Vulkan normal dependency tree has
  no CubeCL CPU/LLVM runtime. Formatting and diff checks passed.
- macOS/Windows execution and HIP are not validated on this Linux/NVIDIA host.
  CUDA/CPU constructors retain their existing behavior; they were not rerun in
  this session. No backend performance benchmark was run.

## Documentation comparison after commit 66355bf

The recommendations below record the comparison before the user's follow-up
decision to ship native Metal and Vulkan. That decision is recorded above.

**Tentative distribution choices:** Linux builds include `vulkan` and therefore
WGSL; macOS builds include `metal` with `metal-native` available for comparison;
Windows uses the default wgpu/WGSL build. CPU remains excluded. The implementation
commit does not change the default to these tentative selections.

### Linux

WGSL and direct SPIR-V both execute through wgpu/Vulkan on a normal Linux GPU.
Direct SPIR-V removes the WGSL/Naga translation stage and exposes native Vulkan
features, including cooperative matrix operations [1]. Those features do not
themselves accelerate Drip's existing scalar f32 image kernels.

The pinned SPIR-V runtime requires buffer device addresses, the Vulkan memory
model with device scope, and storage-buffer storage-class support [3]. A device
that runs WGSL is not necessarily eligible. Retain `DRIP_BACKEND=wgsl` as an
explicit compatibility option. Both paths passed this session's NVIDIA tests;
AMD and Intel remain unvalidated.

**Tentative:** prefer direct SPIR-V on supported devices after the ordinary
preview benchmark and wider driver validation. Documentation alone does not
establish lower latency. Neither path pulls the CPU LLVM JIT into the normal
dependency graph. Their actual distribution sizes have not been measured.

### macOS

Both choices use CubeCL's MSL compiler. `metal` delegates execution to wgpu;
`metal-native` has its own allocation, submission and synchronization code.
Neither requires the CPU LLVM JIT [3].

The native backend PR reports improvements on copy/add microbenchmarks but also
shows its precise transcendental math losing to wgpu's fast math. With matched
fast math it wins those examples. These are upstream microbenchmarks, not Drip
measurements. Maintainers kept `metal-native` opt-in pending broader testing [2].

The published source requires MSL 3.2 for both direct paths. Native additionally
requires the Metal3 GPU family; wgpu/MSL accepts Metal3, Apple7 or Mac2 and probes
compiler support. Ordinary WGSL has a different compatibility envelope [3].
Do not infer the minimum supported macOS from wgpu's general platform list.

**Tentative:** start with `metal` for distribution on supported Macs, keep WGSL
available, and evaluate `metal-native` before promoting it. Native is a useful
performance candidate, not an established winner. Precise-versus-fast math is
also a correctness variable for sigmoid and nonfinite-value handling.

### Frontend and remaining decisions

Drip currently reads presentation data back to the host before uploading it to
the renderer. Either Metal runtime fits that flow without changing GUI behavior.
Future direct sharing with the renderer would be simpler through wgpu's device
and buffer APIs, but merely selecting wgpu does not share devices automatically.
Windows requiring D3D12 presentation does not technically force D3D12 computation;
using it for both is a distribution simplification.

| Status | Remaining work |
| --- | --- |
| Open | Validate Windows/D3D12 and macOS/MSL/native Metal on real machines. |
| Open | Establish minimum macOS/GPU support from the pinned compiler requirements. |
| Open | Compare the existing uniform preview benchmark, matching math policy where possible; keep artifacts outside Git. |
| Open | Measure packaged size and dependency requirements before choosing release defaults. |

### Sources

[1] CubeCL contributors, “Hardware Features,” *The CubeCL Book*. [Online].
Available: https://burn.dev/books/cubecl/core-features/features.html.
The book's datatype table is not treated as an exact 0.11.0 compatibility matrix;
the current WGSL source also supports extensions absent from that table.

[2] D. Cvz and CubeCL maintainers, “feat: metal backend,” pull request 1175,
merged Jun. 25, 2026. [Online]. Available:
https://github.com/tracel-ai/cubecl/pull/1175.

[3] CubeCL contributors, *CubeCL 0.11.0*, published crate source, revision
`52c5086d3c5e73e56b3ee4cb711bdda495a360f0`. Inspected locally: `cubecl-wgpu`'s
`src/backend/vulkan/features.rs`, `src/backend/metal.rs`, `src/compiler/base.rs`;
`cubecl-metal`'s `src/runtime.rs`, `src/compute/context.rs`; runtime Cargo
manifests. [Online]. Available:
https://github.com/tracel-ai/cubecl/tree/52c5086d3c5e73e56b3ee4cb711bdda495a360f0.
