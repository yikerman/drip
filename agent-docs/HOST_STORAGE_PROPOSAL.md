# Host storage and CubeCL ownership

Status: **decided; user accepted the storage design after CPU/WGPU validation
and the uniform benchmark. Checkpoint before this change: `7b7ea5d`.**

## User requirements

- Same-placement edges and fan-out share immutable storage.
- Evaluation materializes a missing CPU/device representation once per output
  within the run. Layout conversion may be necessary at that boundary.
- Fix ownership cleanly; inspect CubeCL's existing memory abstraction before
  introducing Drip infrastructure.
- Keep this design and audit obsolete evaluator complexity. Test WGPU and keep
  a uniform benchmark; the default benchmark runs on WGPU, not CubeCL CPU.
- The user asked whether one concrete layout can cover CPU and GPU. A common
  element layout is possible, while residency/owners remain distinct. This
  change does not alter RGB's existing three/four-lane representations.

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

## Implementation

`HostBuffer<T>` retains CubeCL Bytes behind an Arc and exposes checked typed
slices. Bayer uses it on the CPU; RAW normalization hands over its Vec allocation
once. Uploads use CubeCL's `shared-bytes` controller with a `bytes::Bytes` owner
retaining that Arc. This optional dependency feature avoids a custom allocation
controller and keeps fresh output mutation independent of CubeCL's shared-Bytes
copy-on-write behavior. Mutable access requires exclusive writable storage;
node outputs are fresh allocations. Input borrows remain immutable.

Readback retains returned resident Bytes after checking alignment and element
size. No lazy reads, unsafe adapters, custom allocators, evaluator lifecycle hooks
or macro changes are introduced. RGB retains its existing packed CPU layout and
explicit RGBA conversion; changing it is outside this ownership experiment.
Small coefficients remain ordinary arrays. The evaluator's per-run representation
sharing is unchanged.

Validation covers Vec adoption and upload pointer identity, retained lifetime,
layout rejection, a device/CPU/device Bayer chain with fresh CPU output writes,
and shared downloaded storage across consumers. CPU tests and hardware WGPU
tests pass. Performance follows `BENCHMARK_PROTOCOL.md`; results remain outside
Git. The ownership-only worker run produced bit-identical preview bytes against
the checkpoint; it exercises the removed Bayer upload clone. It does not time
a large CPU Bayer island or prove retaining mapped storage wins for every CPU
consumer.

## Evaluator simplification

Each logical output now owns one record containing its description, native
representation and optional transferred representation. This removes the
separate per-node description map and repeated producer-placement lookup.
Native and transferred owners are released together after the final consumer.

Keep dependency traversal, failure isolation, contracts, transfer reuse and
last-consumer counts. CubeCL protects queued operations, but cannot decide when
Drip's evaluation table should drop its references. Removing those counts would
retain all demanded intermediates until the entire evaluation finishes. This
cleanup adds no persistent image cache, copy-on-read or new synchronization.

References: CubeCL 0.11.0 `cubecl-environment/src/bytes/{base,shared_arc}.rs`,
`cubecl-runtime/src/{client,server/handle}.rs`, CPU/WGPU allocation controllers
and WGPU Vulkan allocation code. Public documentation:
[Bytes](https://docs.rs/cubecl-environment/latest/cubecl_environment/bytes/struct.Bytes.html),
[client readback](https://docs.rs/cubecl-server/latest/cubecl_server/client/struct.Client.html).
Local pinned source takes precedence over docs.rs pages that resolve to prereleases.
