# Node boundary audit

Date: 2026-10-08. Current `cubecl-rewrite` implementation, independently reviewed
by Astra/high. User requirements are recorded separately as R49–R51 in
`TYPE_SYSTEM_REDESIGN_USER_NOTES.md`. This audit does not treat earlier proposals
as requirements.

## Rule

**Requirement:** a node is an ordinary operation plus mechanical DAG bindings.
DAG/eval and the generated adapter own connection checking, dependency traversal,
port materialization, output allocation and value lifetime. The operation owns
its mathematics, parameter-derived constants and algorithm-local scratch.

**Decided:** expose an operation or data product when it has a useful independent
meaning. A helper function, CPU/GPU boundary or kernel pass is not sufficient
reason to make another node. Small functions need not be combined merely because
they are small.

## Findings

| Area | Status | Finding and disposition |
| --- | --- | --- |
| Sigmoid preparation/application | Fixed | One Sigmoid node with settings, image input and image output. Coefficients remain private per-invocation constants. Removed the preparation node, coefficient payload and template edge. Existing controls and curve remain on Sigmoid. |
| Highlight output ownership | Fixed | Opposed reconstruction now fills the adapter-allocated output slice. It no longer allocates a second full image and replaces the first. Bypass copies into the supplied slice. |
| Sigmoid plot | Fixed | A closure prepares the private coefficients once per plotted curve instead of once per point. No persistent cache or new graph value. |
| RAW template bindings | Fixed | Connections resolve declared port names instead of duplicating numeric output indices. No new binding framework. |
| RAW output separation | Retained | Mosaic, site gains, matrix, clipping levels and capture metadata have independent meanings and availability. Export and processing consume different outputs. |
| White balance / clipping levels | Retained | Applying gains to an image and transforming a calibrated clipping value are useful independent operations. Merging them would force an irrelevant levels input or require optional-output machinery. This is a design judgment, not evidence of an existing second levels consumer. |
| RCD and preview reduction | Retained internally | RCD's kernel passes and CFA reduction are local implementation stages. Every node receives the same global context; only descriptors/data change along edges. |
| Matrix, exposure, explicit reduction | Retained | Each is an independently useful user-selected operation. Camera/Color reduction wrappers share their implementation. |
| RAW admission | Retained | Its manual adapter binds an immutable asset or load error before computation. A general macro asset-binding facility solely for RAW would add infrastructure. |
| DAG/eval plumbing | Retained | Transport, dependency traversal, last-use release and failure isolation already live in evaluation. Typed allocation/invocation lives in generated adapters. Nodes do not repeat port transfers. Kernel dispatch and local constants remain ordinary algorithm code. |
| CaptureMetadata GPU representation | Open tradeoff | The custom GPU codec has no production consumer. Removing it would relax the earlier requirement that every payload have equivalent CPU/GPU representations. Kept pending that requirement decision; do not build more such codecs speculatively. |

## Related correctness fixes

**Fixed:** preview labels distinguish requested detail from actual image size.
A producer can ignore spatial detail, and tiny Bayer data can remain unchanged.

**Fixed:** Bayer averaging scales large finite inputs before accumulation, so a
finite mean does not overflow merely because its sum does. The CPU and GPU use
the same kernel, with regressions for positive/negative maximum values and
cancelling values. The existing exponent-shift helper avoids GPU reciprocal
underflow and arithmetic reassociation at those extremes.

Validation results are recorded in `CUBECL_REWRITE.md`.
