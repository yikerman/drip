# Design

- **Decided:** wgpu/WGSL is the computational backend. Software GPU adapters are the fallback; no second production CPU kernel implementation. Host decoding, metadata and file output remain supported.
- **Requirement:** Complete and validate the headless library first; migrate the frontend afterwards, including labels and help.
- **Decided:** Concrete payload families parameterized by CPU/GPU placement, with domain-specific host-readable meaning. Types describe representation; runtime refinements validate coordinate, scene/model, domain and cross-input requirements. Additive color coordinates do not establish original-scene measurements.
- **Decided:** Pure nodes declare concrete input/output payloads and placement. Local declarations generate mechanical registration and binding. Kernel docs state mathematical/physical assumptions and output guarantees.
- **Decided:** DAG owns nodes, connections and structural validation. Eval walks requested dependencies, adapts CPU/GPU placement once per shared value, and releases intermediates after last use. No persistent DAG result cache. Source/resource caching remains explicit.
- **Requirement:** GPU intermediates remain resident across kernels; synchronize only for actual host consumers/completion. Budget and report allocations; fail clearly when a workload cannot fit.
- **Requirement:** Preserve working RAW processing, color management, export, project editing and presentation behavior through the rewrite. Validate real workloads, numerical outputs and invalid contracts.
- **Decided:** RAW source caching includes normalized samples and metadata, shared across preview levels and export. Reload sources replaces that snapshot; downstream numerical outputs remain uncached.
- **Decided:** Frontend views share one evaluation request and the renderer's device. Normal previews sample the resident FP32 buffer; ICC proofing and CPU scopes are explicit host consumers.
- **Open:** Whole-image buffer limits, tiling and a bounded host resource cache. The current GPU budget covers compute buffers and readback staging, not all driver or display allocations.
- **Tentative:** Benchmark evidence now supports GPU scope reductions, removal of the extra host layout copy, then bounded source/prefix reuse. Full recomputation is faster on the GPU, but removing upstream cache reuse regresses some late edits; no new cache policy has been implemented.
