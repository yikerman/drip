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
