# RGB box reduction on CubeCL

Status: decided; implemented and validated on CubeCL CPU and hardware WGPU.

Requirement: port the straightforward existing CPU nodes first. Scope changes
remain a separate discussion. Preserve concrete payloads, contracts and GUI behavior.

Camera RGB and Color RGB reduction now use device ports and one shared CubeCL
kernel in `node/shared_kernel.rs`. Their node modules contain the contract and
call a shared dispatcher. Evaluation owns upload/readback. The former standalone
CPU computation is removed; CubeCL CPU executes the same kernel as WGPU.

The factor parameter still determines reduction independently of global preview
scale. Partial blocks retain all available samples. Sample encoding and
interpretation are preserved, including explicitly averaging encoded RGB.

Portable f32 replaces the former f64 accumulation. The kernel reuses the Bayer
reducer's power-of-two scaling for large finite samples. This prevents overflow
without requiring device f64. Cancellation error scales with input magnitude;
results are not promised to match f64 bit-for-bit. No additional precision mode
or algorithm selection is introduced.

Validation compares both payloads with a test-only f64 reference over odd/narrow
extents, partial blocks, identity and maximum factors, HDR and nonfinite values.
A device/exposure/reduction/exposure chain checks one initial upload and one final
readback. The existing uniform default-worker benchmark does not contain these
explicit RGB reduction nodes; it cannot measure this port's performance.

CPU library/integration/doc tests, WGPU algorithm tests, Clippy for drip/drip-gui
and formatting checks passed.
