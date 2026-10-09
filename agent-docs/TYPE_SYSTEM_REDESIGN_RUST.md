# Rust formulation: typed payloads and checked connections

Date: 2026-10-08. Status: **tentative proposal**, following R09–R14 in
[the user record](TYPE_SYSTEM_REDESIGN_USER_NOTES.md). This develops the
[practical contract proposal](TYPE_SYSTEM_REDESIGN_PRACTICAL.md); it does not
treat the current implementation as authoritative or change production code.
Rust blocks are API sketches; buffer definitions, method bodies, and registry
plumbing are omitted. The compiler probes described below check selected type
signatures, not a completed graph implementation.

Follow-up: [independent port payloads](TYPE_SYSTEM_REDESIGN_PORT_PAYLOADS.md)
clarifies multi-output nodes, independently reusable matrices/LUTs/calibration
values, and payload extension. `Value<P>` below is one port value, not a
node-wide image-and-auxiliary-data bundle.
Following R21–R23, the CPU checking/propagation function is called `contract`;
the port-payload document proposes `#[node(contract = ...)]` declarations.

## Feasible guarantee

Connect time is an application event: the user or program requests an edge
while Drip is running. It is distinct from compiling Rust and from evaluating
image pixels. Rust can enforce access to an API that checks that request and
creates an edge only on success. Rust does not itself prove arbitrary
relationships between camera profiles, dimensions, or runtime parameters.

Proposed guarantee: **every committed connection satisfies its structural
and descriptor-based contracts in the current checked graph configuration.**
The configuration includes parameters, descriptor-relevant context, source
descriptions, and execution placements. Changing those inputs requires
revalidation. Explicitly declared checks on computed values remain evaluation
conditions, not facts established by connecting an edge.

This guarantee requires describing an output before computing its pixels.
Keeping metadata on the CPU helps access it; that alone does not make future
metadata knowable. The main design question is therefore which outputs admit
a cheap, exact description from input descriptions and parameters.

## Concrete storage, host descriptions

An associated-type mapping names concrete representations [1]. Schematic
Rust, with supporting buffer/descriptor definitions omitted:

```rust
trait Payload: 'static {
    type Cpu;
    type Gpu;
    type Desc: Clone; // Host description, including interpretation.
}

struct BayerF32;
impl Payload for BayerF32 {
    type Cpu = CpuPlaneF32;
    type Gpu = GpuPlaneF32;
    type Desc = BayerDesc;
}

struct XTransF32;
impl Payload for XTransF32 {
    type Cpu = CpuPlaneF32;
    type Gpu = GpuPlaneF32;
    type Desc = XTransDesc;
}

struct CameraRgbF32;
impl Payload for CameraRgbF32 {
    type Cpu = CpuRgbF32;
    type Gpu = GpuRgbF32;
    type Desc = CameraRgbDesc;
}

// Evaluator-owned bookkeeping; this enum is not a kernel argument.
enum Storage<P: Payload> {
    Cpu(P::Cpu),
    Gpu(P::Gpu),
}

struct Value<P: Payload> {
    desc: std::sync::Arc<P::Desc>, // Ordinary host allocation.
    storage: Storage<P>,         // CPU allocation or GPU resource handle.
}
```

For example, `CpuPlaneF32` can own `Vec<f32>`, whereas `GpuPlaneF32` owns a
typed view of a device allocation. `CpuRgbF32` can own `Vec<[f32; 3]>` while
the GPU form uses a different declared stride. These are concrete layouts;
there is no algebraic object passed into a pixel kernel. `Payload` binds
representations and their description, not a universal interface for doing
image mathematics.

`BayerDesc` contains logical extent, phase, camera-coordinate identity, and
the declared value convention. `XTransDesc` contains its sampling information.
`CameraRgbDesc` contains extent, camera coordinates, and value convention.
A separate `ColorRgbF32` family can use `ColorRgbDesc { extent, interpretation }`,
with `RgbInterpretation { space, encoding, scale }` as an explicit host value.
Interpretation remains the basis of compatibility. Some facts are carried by
the nominal payload type and others by its runtime description; `Desc` does
not mean that interpretation was discarded or replaced by storage layout.
Canonical runtime identities handle loaded camera profiles and RGB spaces;
there is no Rust type for every image size or camera profile.
Optional information that a contract does not consume need not be supplied.
An unknown field is not a wildcard that satisfies a check requiring that field.

Descriptions are immutable per value/configuration. Logical extent has one
authoritative description; storage also records physical capacity/stride as
needed to validate the view. Pairing a description with storage is private
to validated import, allocation, and node-output paths. A freely constructible
`Value { arbitrary_desc, arbitrary_buffer }` would defeat this invariant.
Rust privacy can enforce that API boundary [2]; it cannot verify that a
trusted producer truthfully describes its algorithm.

CPU/GPU transport preserves the logical payload and interpretation. Padding
and layout conversion may differ. A change from f32 to f16 is a numerical
conversion, not a lossless movement of the same `F32` payload. The earlier
bijection requirement is meaningful for supported logical values, not for
every bit pattern in padding or every possible GPU allocation.

## Fixed nodes and CPU contract functions

`BayerDemosaic` and `XTransDemosaic` remain different node types. Each declares
its concrete port families and one contract with the node. A small CPU
function checks descriptors and constructs output descriptors:

```rust
fn bayer_demosaic_contract(
    context: &ContractContext,
    params: &BayerDemosaicParams,
    input: &BayerDesc,
) -> Result<CameraRgbDesc, ContractError>;
```

This function can check the selected algorithm's size and value-convention
requirements and establish its output extent and response coordinates. It
does not inspect image samples, select another demosaic method, or allocate
image storage. Contracts, descriptor propagation, and computational helpers
are defined once with the node; registration/adapters are mechanical.

Multi-input nodes receive tuples or named structs of concrete descriptions.
For a blend these might be two `ColorRgbDesc` values and a mask description;
its rule compares coordinates, encoding, and the required grid relationship.
This is ordinary Rust checking of ordinary values, not dependent typing.

Multi-output nodes declare named collections of independently typed ports.
Each output has its own descriptor, storage, placement, and consumers. A
matrix or LUT produced by a node is computational data on its own output;
its coefficients or samples are not hidden inside an image descriptor.
`Payload` is an extensible implementation/registration interface, not a
closed enum of everything a node could ever return.

A description is available before the corresponding `Value<P>` exists. The
graph can retain these small descriptions without caching image buffers.
Prepared coefficients can likewise be derived from descriptions and known
parameters. Preparation that depends on computed scalar/matrix values occurs
later, before the relevant computation.

## Placement and transport belong to the evaluator

Each node instance is bound to a supported implementation/placement when its
ports are exposed. Its port requirements say CPU or GPU, and identify the
bound device where necessary. Placement is known at connect time. It does
not require choosing between WGSL and CubeCL in this design.

A GPU entry point receives the GPU representations directly. For example:

```rust
fn run_bayer_demosaic(
    context: &GpuDispatchContext,
    params: &BayerKernelParams,
    input: &GpuPlaneF32,
    output: &mut GpuRgbF32,
) -> Result<(), DispatchError>;
```

The evaluator supplies validated constants, allocates the private output,
provides the required views, and handles dependencies and submission. The
computational entry point implements its algorithm. It does not match on
`Storage`, choose CPU versus GPU, upload, download, or rewrite interpretation
metadata. The evaluator uses the node's declared contract function for
that metadata. Writing an
evaluator-owned fresh output is an implementation of the pure value function;
it does not authorize mutation of an upstream value shared by other consumers.

If an algorithm supports multiple placements, registration exposes the chosen
entry point. This does not require separate handwritten computational kernels;
the backend design must still satisfy the one-source-kernel requirement. A
node need not advertise an execution backend that has no implementation.

| Connection | Planned evaluator action |
| --- | --- |
| CPU output → CPU input | Share/borrow the immutable CPU value. |
| GPU output → GPU input on the same device | Pass the device view, preserving residency. |
| CPU output → GPU input | Upload, including any declared storage-layout packing. |
| GPU output → CPU input | Await the producer and download/unpack before invoking the consumer. |

An unavailable representation adapter is reported while connecting/planning.
Allocation limits, transfer failure, and device loss remain operational
evaluation failures. Initially one bound GPU is sufficient; cross-device
transfers need not be an implicit capability.

The graph records transfer requirements, not image copies. Evaluation may
reuse a resident copy across several consumers within that evaluation and
release it after its last use. Persistent result caching is a separate policy.
`Storage` above represents one materialization; evaluation may retain both a
CPU and a GPU materialization of the same immutable logical value while each
has consumers. Their descriptions stay on the host. Only the fields needed
by a kernel are also supplied as concrete kernel constants.
No hidden color conversion, CFA conversion, or precision reduction belongs in
these transfer adapters.

## A checked edge construction API

The typed Rust API can distinguish payload families and physical placements:

```rust
// Fields and constructors are private. P identifies logical payload; L placement.
struct Output<P: Payload, L> { /* graph-owned handle */ }
struct Input<P: Payload, L> { /* graph-owned handle */ }

impl Dag {
    fn try_connect<P: Payload, From, To>(
        &mut self,
        source: Output<P, From>,
        target: Input<P, To>,
    ) -> Result<EdgeId, ConnectError>;
}
```

An `Output<BayerF32, Cpu>` can connect to `Input<BayerF32, Gpu>` after the
runtime contract check and transfer planning. An `Output<XTransF32, Cpu>`
cannot satisfy that typed call. Different RGB spaces can still have the same
`ColorRgbF32` Rust family, so their coordinate/encoding compatibility must be
checked at connect time. `PhantomData` can carry port type parameters [3],
but a marker alone is not evidence of validation.

The GUI and serialized graphs necessarily use dynamic node/port identities.
Provide `try_connect_ids(output_id, input_id)` through the same checked path.
It verifies nominal payload identity before applying the concrete node's
descriptor checks. Type erasure stays in the graph registry/adapters; it is
not passed to kernels. Rust's `TypeId` is usable for in-process identity, not
as a persistent format identifier [4].

The graph owns its private edge records. An edge is created only by a
successful checked edit; no public unchecked insertion, descriptor mutation,
or deserialization directly into accepted edge records is provided. Handles
are checked for graph ownership and live node/port generation. An `EdgeId`
names a graph-owned connection; it is not an independently reusable proof.

`try_connect` stages an edit and then:

1. Checks handles, port direction/occupancy, payload families, and acyclicity.
2. Checks the source description against the target's fixed requirements.
3. Recomputes affected output descriptions and checks existing downstream
   connections, including relationships with other connected inputs.
4. Verifies supported placements/adapters and records transfer requirements.
5. Commits the edit only if the checks pass; otherwise leaves the graph intact.

The staging/revalidation operation concerns graph records and small host
descriptions. It does not duplicate image buffers or evaluate the DAG's pixels.

An incomplete node is not an incompatible connection. For a two-input blend,
the first edge checks that input's own contract. The second edge must also
pass the relation to the first input before it is accepted. Missing required
inputs keep the node unevaluable. If its output cannot yet be described, a
downstream connection requiring that description is unavailable. All-input
checks need not become evaluation-time checks merely because the GUI adds
edges individually. A batch connection/edit can handle jointly supplied inputs.

## Changes after connection

Checking only the first insertion is insufficient. For example:

```text
RGB conversion targeting Rec.2020 → Rec.2020 luminance
```

Changing that conversion's target to another space invalidates the existing
edge. The parameter update must use the same checked transaction as edge
creation. Reject the update, or explicitly edit/disconnect the affected edges
in the same transaction. Do not retain a stale edge marked valid.

The same rule applies to source replacement, profiles, crop parameters,
placement changes, and descriptor-relevant global context. This is a real
strictness tradeoff: changing a source can require changing several nodes or
edges together. A UI can stage a batch edit without admitting an invalid
committed connection. Serialized graphs are loaded through validation too.

Each evaluation uses an immutable checked configuration. Ordinary ownership
(`&Dag` versus `&mut Dag`) or an immutable snapshot can prevent edits from
changing a running evaluation; a whole typestate hierarchy is unnecessary.
Prepared descriptors/transport plans must belong to that same configuration.

In particular, `eval(dag, global_context, target)` cannot preserve the guarantee
for arbitrary new context fields that change port meanings or dimensions.
Bind those fields while checking the graph. A different preview/export
configuration must be checked before it runs. Execution-only context, such as
an allocator or submission handle, need not change the semantic contract.

## What can be checked when?

| Condition | Proposed point of enforcement |
| --- | --- |
| Concrete representation and fixed nominal family in handwritten Rust wiring | Rust compile time, plus dynamic checks for GUI/loaded wiring. |
| Declared Bayer versus X-Trans, camera versus defined RGB, encoding, profile endpoints | Connect time, with descriptions available. |
| Crop phase, parameter-derived extent, equal dimensions for a strict elementwise blend | Connect time after propagating descriptions. |
| Invalid literal parameters, such as a known zero normalization denominator | Parameter admission/connect time. |
| Required CPU/GPU representation and transfer adapter | Connect-time planning; transfer during evaluation. |
| Missing required input | Incomplete node; cannot evaluate until supplied. |
| Unknown header/profile metadata needed by a connection | Resolve the description first; connection returns `NeedsDescription`, not success. |
| A denominator or matrix property depending on computed values | Evaluation, before the consuming operation uses it. |
| A condition on actual pixels that the algorithm genuinely requires | Evaluation, unless the producer establishes it by construction. |
| Allocation, I/O, and device failures | Evaluation/runtime failures, distinct from semantic incompatibility. |

For ordinary raw pipelines, a source-probing/import step can read dimensions,
CFA information, and declared color metadata before pixel processing.
Profile endpoints can be loaded similarly. A filename without this information
does not establish a compatible output description. The probe may involve I/O
or decoding; it is not guaranteed free. Processing nodes do not guess the
missing information.

Source metadata and decoded storage must refer to the same actual input.
Use a bound input snapshot/version where available; validate the decoded
description at the import boundary. An external file can change without a
graph edit, and a path or cached timestamp alone is not proof of immutability.
A mismatch requires explicit revalidation, before presenting the new value
as the old checked payload. This is a boundary check, not repeated inspection
of every internal image.

Source binding also resolves the nominal payload family. For example, import
can bind an asset with a declared Bayer header to a typed Bayer source before
exposing its output port. The decoder must subsequently satisfy that bound
description. The demosaic node is never asked to infer the source family.

There are two different reasons an output description may be unavailable:

- **Not obtained yet:** a RAW header or selected LUT's declared domain. Obtain
  it before connecting. Deferring this by default would weaken ordinary checks.
- **Depends on computation:** a crop extent selected from pixel contents, or
  transform coordinates determined by a computed choice. Such an operation
  cannot promise arbitrary downstream compatibility before those results exist.

Prefer a specified output contract where it is natural: for example, a crop
node with an explicit rectangle has a known output extent. Do not replace a
requested content-dependent algorithm with a different one merely to make
the checker simpler.

For genuinely computed descriptions, the strict option is to materialize the
necessary result before connecting. If that is impractical, a later extension
can admit an explicitly pending connection whose remaining conditions are
checked during evaluation. A pending connection must not be represented as
already checked. This relaxation is not needed for ordinary Bayer/X-Trans,
RGB-domain, profile-endpoint, and parameter-derived shape checks.

Some value-dependent evaluation conditions remain even with strictly checked
descriptions. A matrix may have known coordinate endpoints but computed
coefficients; an operation that needs its inverse must handle or reject a
singular matrix when the coefficients exist. This does not require deferring
the matrix's coordinate compatibility check.

The CPU owns graph admission decisions. If an essential condition concerns
GPU samples, it cannot be established by reading host descriptions alone.
A GPU reduction can report a small status/result to the CPU, but that still
introduces synchronization. Prefer an algorithm's well-defined numerical
handling where appropriate; do not silently replace a required rejection by
clamping simply to avoid the check.

## Proposed disposition

| Status | Item |
| --- | --- |
| Requirement | Concrete CPU/GPU buffers; host-accessible descriptions may be physically separate. |
| Requirement | Separate algorithms have separate nodes; no guessing or substitution inside nodes. |
| Requirement | Placement is known at connection; the evaluator owns movement and supplies the required representation. |
| Tentative | Associated types bind each payload family to concrete CPU/GPU representations and a host descriptor. |
| Tentative | Strict descriptor checks at connection; unavailable required descriptions prevent connection. |
| Tentative | All descriptor-affecting edits are checked transactions; evaluations use the same bound configuration. |
| Tentative | Keep explicitly identified value-dependent conditions at evaluation; no general physical-model inference. |
| Open | Whether any initial algorithm truly needs connections with computed, unavailable descriptions. Add pending edges only for a demonstrated need. |
| Open | Exact representation adapters and backend APIs; this proposal does not select WGSL or CubeCL. |

## Validation of the formulation

Compiled an isolated signature probe with `rustc 1.99.0`, edition 2024:

- Associated CPU/GPU/description types and the evaluator-owned `Value<P>` compile.
- Typed CPU → GPU and GPU → GPU calls compile for the same payload family.
- An X-Trans output passed to a Bayer input is rejected by the Rust type checker.
- Constructing private port fields or the private edge-ID tuple field from
  another module is rejected.

The probe used placeholder GPU handles and an unimplemented connection body.
It verifies the Rust typing/privacy mechanisms only. Runtime validation,
transactional edits, descriptor correctness, kernels, and transfers remain
proposed design work. No production code or original project logs changed.

## References

[1] Rust Project, “Associated items,” *The Rust Reference*, accessed Oct. 8,
2026. [Online]. Available:
[reference page](https://doc.rust-lang.org/reference/items/associated-items.html).

[2] Rust Project, “Visibility and privacy,” *The Rust Reference*, accessed
Oct. 8, 2026. [Online]. Available:
[reference page](https://doc.rust-lang.org/reference/visibility-and-privacy.html).

[3] Rust Project, “PhantomData,” *Standard Library Documentation*, accessed
Oct. 8, 2026. [Online]. Available:
[standard library page](https://doc.rust-lang.org/std/marker/struct.PhantomData.html).

[4] Rust Project, “TypeId,” *Standard Library Documentation*, accessed
Oct. 8, 2026. [Online]. Available:
[standard library page](https://doc.rust-lang.org/std/any/struct.TypeId.html).
