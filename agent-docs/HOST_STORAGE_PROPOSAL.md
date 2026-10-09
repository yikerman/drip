# Host storage and CubeCL ownership

Status: **tentative; investigation only, no storage implementation changed.**

## User requirements

- Same-placement edges and fan-out share immutable storage.
- Evaluation materializes a missing CPU/device representation once per output
  within the run. Layout conversion may be necessary at that boundary.
- Fix ownership cleanly; inspect CubeCL's existing memory abstraction before
  introducing Drip infrastructure.

## Findings

Checked the locally pinned CubeCL 0.11.0 source. `server::Handle` already owns a
backend-managed allocation, including CPU-runtime allocations. Clones retain
the allocation. `Bytes` owns host-accessible transfer storage from native,
pinned or mapped allocations; `from_elems` consumes a Vec without copying.
`Bytes::shared()` enables reference-counted clones. Ordinary `Bytes::clone()`
does not universally guarantee that property. Shared mutation invokes copying,
so fresh mutable output allocation and published immutable storage must remain
distinct in the ownership path.

`Client::create(Bytes)` retains upload data in the queued operation.
`Client::read_one` returns owned `Bytes`. The CPU backend returns a view retaining
its pool allocation; WGPU readback retains mapped staging storage. Drip's Bayer
bridge currently clones a Vec before `create` and copies returned Bytes into a
Vec. These extra host copies are not required by graph sharing or purity.
RGB additionally converts packed RGB to/from device RGBA; that is a separate
layout operation.

`Client::read_lazy` returns Bytes backed by a device handle and downloads on
first host access. This is deferred readback, not a universal coherent CPU/GPU
allocation. It would move waits/errors into access unless evaluation explicitly
materialized it. There is no need to introduce lazy reads for the present fix.

The WGPU Vulkan backend can use host-visible device-local allocations on
integrated GPUs. The inspected CUDA path uses separate device storage and pinned
host storage. Neither supplies a portable, directly CPU-dereferenceable typed
GPU buffer that replaces both Handle and Bytes in Drip's current model.

## Proposed direction

Reuse CubeCL's owners and allocation lifecycle. If typed host storage is needed,
keep its responsibility to element-layout validation and slice access. Do not
add an allocator, general residency manager or implicit transfer on node reads.
Validate sharing, queued-transfer lifetime, fresh output mutation and readback
alignment before choosing the adapter. Keep the evaluator's existing per-run
representation sharing. No code or borrow-checker workaround is justified yet.

References: CubeCL 0.11.0 `cubecl-environment/src/bytes/{base,shared_arc}.rs`,
`cubecl-runtime/src/{client,server/handle}.rs`, CPU/WGPU allocation controllers
and WGPU Vulkan allocation code. Public documentation:
[Bytes](https://docs.rs/cubecl-environment/latest/cubecl_environment/bytes/struct.Bytes.html),
[client readback](https://docs.rs/cubecl-server/latest/cubecl_server/client/struct.Client.html).
Local pinned source takes precedence over docs.rs pages that resolve to prereleases.
