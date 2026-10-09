# Desktop compute backend selection

## User requirements

- **Requirement:** expose backend build options and environment selection,
  validate and commit this implementation, then research distribution choices.
- **Requirement:** do not distribute the CPU runtime for now; its LLVM JIT is
  too large. Keep it available for development.
- **Requirement:** Windows needs D3D12. Compare Linux WGSL versus direct
  SPIR-V, and macOS wgpu/MSL versus native Metal before choosing distribution
  defaults. This work does not target headless distribution.

## Implementation

- **Decided:** `drip` features are `wgpu` (default), `vulkan`, `metal`,
  `metal-native`, `cuda`, `hip`, and `cpu`. The crate README documents their
  matching `DRIP_BACKEND` values. `wgsl` aliases `wgpu`; `dx12` selects the
  Windows WGSL path explicitly.
- **Decided:** WGSL, SPIR-V and MSL selectors name explicit compiler types.
  Enabling a feature must not silently change an explicit compiler selection.
  `wgpu` uses D3D12 on Windows and automatic graphics API selection elsewhere.
  No automatic CPU or vendor-runtime fallback.
- **Decided:** keep the existing WGSL default until the comparison is complete.
  GUI rendering, evaluator transport and kernels are unchanged.
- **Open:** distribution defaults for Linux/macOS, dependency size, and native
  platform validation. No performance claim follows from exposing a selector.

## Session validation

- Vulkan-enabled `drip`/`drip-gui` build and all-target Clippy passed.
- Library unit tests passed. Existing algorithm, opposed-highlight reference,
  and hybrid-transfer tests passed on hardware with both `DRIP_BACKEND=vulkan`
  and `DRIP_BACKEND=wgsl`, using the same Vulkan-enabled test binaries.
- `drip --no-default-features` compiles. The Vulkan normal dependency tree has
  no CubeCL CPU/LLVM runtime. Formatting and diff checks passed.
- macOS/Windows execution and HIP are not validated on this Linux/NVIDIA host.
  CUDA/CPU constructors retain their existing behavior; they were not rerun in
  this session. No backend performance benchmark was run.
