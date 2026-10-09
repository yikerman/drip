# Concrete payloads and shared transport

Status: decided; implemented and validated on CubeCL CPU and WGPU.

## User requirements

- Simplify the type system and macro after accepting CubeCL-backed host storage.
- One source of truth; report inherent difficulties instead of adding workarounds.
- Keep concrete Rust types. Uniform movement does not mean turning every payload
  into a homogeneous buffer or replacing structs with opaque bytes.
- Replace incompatible fields locally (UTF-8 text and ICC data can be byte vectors).
  A small local adapter is acceptable.
- Contracts reject inherent logical errors, not preferred processing order.
  Check connections and data-dependent boundaries; trust internal code afterwards.

## Decisions

- A payload declares one `Data` type and its description. Remove the independent
  per-payload device type. `DeviceBuffer<P>` supplies the shared CubeCL handle.
- Keep arrays as arrays: matrices and coefficients need no wrapper just to match
  images. Images use typed CubeCL-owned host storage; packed RGB is `[f32; 3]`
  on both sides. Remove RGB padding/unpacking.
- Shared code allocates device storage, submits uploads and receives readbacks.
  Payload adapters describe byte size and expose/reconstruct their concrete type.
- `HostBuffer<T>` remains a narrow typed owner for CubeCL `Bytes`. It validates
  element alignment and size and retains allocations through readback/uploads.
  It is not another payload schema. `Cpu<P>` is a port placement marker, not an
  owning container. Merging the two would conflate ownership with port contracts.
- Capture settings remain the single `drip_raw::Metadata` struct. Its text fields
  are UTF-8 byte vectors. The existing local adapter packs numeric fields and
  vector contents; pointer/capacity headers never cross the device boundary.
  The RAW decoder normalizes text once; export borrows UTF-8 slices.
- The macro only wires inputs, output storage, invocation and publication.
  Ordinary Rust `Read::bind` and `OutputSlot` own shared binding/validation logic.
  Interpretations, optional input semantics and global context remain unchanged.

## Limitation

CubeCL 0.11.0 owns transferred bytes and device allocations, but does not
recursively transfer arbitrary Rust structs containing vectors. A byte vector
has compatible contents, not a device-valid Rust pointer header. Preserve the
small metadata adapter; no recursive transport derive or generic serialization
framework is warranted here. Host and device ownership remain distinct even
where element layouts match.

## Validation

Use the existing CPU/WGPU algorithm references, hybrid/fan-out and graph contract
tests, GUI behavior tests and `BENCHMARK_PROTOCOL.md`. Results and LaTeX artifacts
belong outside Git. Host timing measures completed worker requests, not isolated
GPU kernel time.

CPU/RAW/macro and GUI tests, WGPU backend tests, formatting and Clippy passed.
The uniform worker run and output comparison are in the external benchmark
archive, `2026-10-09-latency/results-simplified/` (report, manifest and LaTeX PDF).
The measured source snapshot and shared driver are archived alongside it.
