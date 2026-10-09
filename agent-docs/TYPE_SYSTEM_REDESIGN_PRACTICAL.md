# Type-system redesign: practical enforcement scope

Date: 2026-10-08. Status: **tentative proposal** responding to user follow-up
R01–R08 in [the user record](TYPE_SYSTEM_REDESIGN_USER_NOTES.md).

Follow-up: [the Rust formulation](TYPE_SYSTEM_REDESIGN_RUST.md) develops
strict connect-time descriptor checks, typed CPU/GPU ports, and evaluator-owned
transport. It supersedes this document's default of deferring unknown required
metadata to evaluation; unresolved descriptions instead prevent connection
unless a specific relaxation is adopted.

This narrows the enforcement proposed in [the first study](TYPE_SYSTEM_REDESIGN_2026-10-08.md).
The first study's mathematical distinctions remain useful when deciding which
checks deserve implementation. Its general tracking of physical models and
evidence is outside this proposed initial scope.

## Target

Detect a deliberately incomplete set of ordinary contract violations.
Every hard rejection must identify a concrete contradiction with a selected
node contract. Passing means that these checks found no error; it does not
certify a physically accurate or well-designed processing pipeline.

In particular, distinguish two choices:

- Leaving an error class unchecked reduces coverage and is acceptable here.
- Rejecting a valid use because its order or outcome looks unusual changes
  the policy and is not authorized by the narrowed objective.

The remaining challenge is defining “ordinary errors.” Use representative
bad connections and unusual valid connections to choose the checks. “Most”
is a coverage target to evaluate against those cases, not an established
property of this proposal.

The initial scope is a finite set of descriptive fields and local checks.
Include a rule when it detects an ordinary mismatch, its inputs can be
supplied without inferring physical truth from pixels, and its metadata can
be maintained by a small local rule. This keeps coordinate, encoding, CFA,
and calibration-domain mistakes in scope while leaving statistical fidelity
and ideal processing order out.

## Findings from actual modules

Inspected darktable source at `733bd69f32cac7ff5e41025115942772add1f088`
and vkdt 1.0.0 at `afcc804898715d05952066718f4e4c0eba21ed76` [1], [2].
These are evidence about concrete operations, not specifications that Drip
must reproduce. The proposed checks below are our design conclusions.

One correction to the premise: vkdt already carries CFA, black/white levels,
camera conversion, color primaries, and transfer-curve metadata in
[`dt_image_params_t`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/module.h#L52).
Its generic [connection compatibility check](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/connector.inc#L85)
compares channel and storage-format tokens; it does not compare those color
interpretations. Individual modules also inspect metadata. The useful
improvement is systematic enforcement of the interpretations each operation
consumes, rather than merely adding metadata that vkdt lacks.

| Inspected operation | Evidence in the implementation | Practical Drip contract and an error it catches |
| --- | --- | --- |
| darktable [`rawprepare`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/rawprepare.c#L336); vkdt [`denoise`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/denoise/main.c#L139) | Raw preparation subtracts channel black offsets and divides by a scale. vkdt updates output black/white metadata to zero/one. | Distinguish source code values from black-relative values in a declared scale. A node specifically applying the source black/white calibration rejects already normalized input. A general numeric rescale remains valid. |
| darktable [`demosaic`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/demosaic.c#L1494); vkdt [`hotpx`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/hotpx/readme.md) | darktable corrects Bayer/X-Trans method mismatches, including those from copied presets. vkdt documents `hotpx` as Bayer-only. | Separate Bayer and X-Trans demosaic nodes. Each checks its required CFA family and current sampling phase. Reject X-Trans into the Bayer node; do not substitute a different algorithm. |
| vkdt [`hilite`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/hilite/main.c#L10) | Non-CFA input causes a bypass. Its [input contract](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/hilite/readme.md) specifies normalized mosaic data. | A sensor highlight-reconstruction node requires CFA samples and a compatible clipping-level convention. Reject demosaiced RGB instead of silently doing nothing. This says nothing about other highlight algorithms. |
| darktable [`colorin`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/colorin.c#L1544) | The code distinguishes a camera-to-XYZ DNG ForwardMatrix from an XYZ-to-camera matrix, despite their identical matrix dimensions. | A color transform carries declared input and output coordinates. Reject a reverse-direction matrix or a profile whose declared input coordinates differ from the image. A generic numeric matrix multiply has a weaker contract. |
| vkdt [`filmcurv`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/filmcurv/main.comp#L56) | Color modes 0 and 2 convert using fixed Rec.2020/XYZ coordinates. Mode 1 just applies the channel curves. | Expose the different algorithms separately. The fixed Rec.2020 variants require those coordinates with identity encoding; the channel curve has a weaker contract. An algorithm prepared with working-space transforms need not inherit the fixed basis. |
| vkdt [`mask`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/mask/main.comp#L27) | Mode 0 uses Rec.2020 luminance; mode 3 averages Bayer blocks. Both produce scalar masks. | Separate Rec.2020 luminance extraction from Bayer brightness estimation. They can feed reusable scalar selection arithmetic. Each extraction has one input contract; a raw brightness proxy is not thereby CIE luminance. |
| vkdt [`colenc`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/colenc/main.comp#L19) | The sRGB path converts from linear Rec.2020 and then applies the sRGB encoding curve. | Separate matrix conversion from the specified encoding operation. The matrix declares its coordinate endpoints; sRGB encoding requires identity-encoded input and preserves its primaries. Feeding encoded output back to that encoder violates its contract. |
| darktable [`colorbalancergb`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/colorbalancergb.c#L588), [`channelmixerrgb`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/channelmixerrgb.c#L2107), [`sigmoid`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/sigmoid.c#L394) | These obtain transforms from the working profile; color calibration also obtains the input profile. | Each exposed algorithm declares the encoding and matching profile data it consumes. Prepare matrices for supported working spaces as concrete parameters. Uninterpreted camera channels cannot stand in for a declared working RGB space. |
| darktable [`lut3d`](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/lut3d.c#L1140) | It selects the LUT application profile and, when both profiles are available, converts working RGB into that domain and back around the lookup. | A LUT declares its input/output interpretation and numeric domain. The lookup node rejects a direct mismatch; explicit conversion nodes can establish the required domain. Table dimensions alone are insufficient. |
| vkdt [`grade`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/grade/main.comp#L23) | The shader applies channel gain, lift, and guarded power, without a color-space matrix. | A generic channel-arithmetic node need not require standard color primaries. Camera RGB is a valid input to that weaker operation. This is a concrete counterexample to forbidding all grading before camera conversion. |

Two checks also follow from composition. Cropping CFA data must update its
phase: darktable's [raw preparation does so](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/rawprepare.c#L301).
A blend defined to combine values in the same coordinates must compare both
inputs' interpretations and their required grid relationship. Neither check
needs to infer whether the images depict the same subject. A numeric multiply
or a blend with declared conversions can have a different contract.

Do not copy restrictions from UI module names. For example, vkdt
[`denoise`](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/denoise/readme.md)
supports both mosaic and dense RGB input and also performs raw preparation.
“Denoise must precede demosaic” would reject a supported operation. Drip can
give the distinct raw and dense-image algorithms separate nodes, with shared
helpers where the computations coincide. A UI module name does not establish
that its modes are one algorithm.

## Upstream tradeoffs we need not inherit

These are design alternatives drawn from inspected source. The motivation
for a pattern is an inference unless the source states it. An upstream
fallback or fixed convention can be reasonable for its application without
being necessary in Drip.

| Observed pattern | Alternative under explicit contracts |
| --- | --- |
| vkdt stores one `img_param` per module; its [comment](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/module.h#L117) acknowledges the per-connector issue. | Attach each interpretation to the corresponding output value. An image, mask, sampled color, and curve visualization need not share one image descriptor. Multi-input operations compare the relevant inputs instead of inheriting one module's metadata indiscriminately. |
| vkdt highlight reconstruction [bypasses non-CFA input](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/hilite/main.c#L10). darktable demosaic [substitutes methods](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/demosaic.c#L1502) after a Bayer/X-Trans preset mismatch. | Give each algorithm its own node and contract. An incompatible connection fails that contract. Any future preset or graph-construction convenience must produce explicit node choices; the processing node does not guess, substitute another algorithm, or silently bypass. |
| darktable color calibration reads ambient input/working profiles and [checks other module instances](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/channelmixerrgb.c#L1980) for WB/CAT warnings. | Pass the required transforms, neutral/white values, and stated conventions through matched inputs, parameters, or immutable context bound to that input. Preparation resolves those values before the pure kernel runs. A front end can assemble defaults; the node need not inspect a module with a particular name or count CAT instances. This checks declared conventions, not complete WB history. |
| vkdt raw preparation is bundled into `denoise`; darktable color calibration bundles adaptation and channel mixing. | Define focused operations and their contracts separately. A UI may group them. Normalization should be available independently of denoising; adaptation and creative channel mixing have distinct contracts even where they share matrix arithmetic. Shared helpers do not require separate image buffers. |
| Some vkdt color kernels assume Rec.2020; darktable's inspected color modules prepare matrices from the working profile. | Either expose a fixed-space kernel with that precise contract, or prepare concrete matrices for supported spaces. A general Drip operation need not inherit an upstream hardcoded basis. It also need not support every possible profile on day one. |
| darktable LUT 3D [converts into one LUT application profile and back from that same profile](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/lut3d.c#L1152). | Let a LUT declare distinct input and output interpretations. A look LUT may use the same space at both ends; a technical LUT may change coordinates or encoding. Any following conversion starts from the actual output interpretation. A table is not assumed to be a same-space look merely because that is a common UI use. |

A separate source example shows the limit of trusting annotations: in the
inspected vkdt revision, `colenc` [writes its selected output primaries to
metadata](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/colenc/main.c#L5),
but the shader's [custom-primaries branch is an unimplemented placeholder](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/colenc/main.comp#L20).
This is a specific inspected code path, not a claim about every release or
the application's normal pipeline. A checker cannot make a promised
conversion happen. Drip must implement a selected conversion, reject an
unsupported selection, or define an explicit assignment of new meaning.
Producer declarations and their implementation must agree.

For LUT application with conversion around it, the composition is:

```text
RGB in A -> convert A to LUT.input -> LUT -> convert LUT.output to A
```

Each step has a concrete representation and declared endpoints. A UI may
present the composition as a group; lookup remains a separate operation with
its own input contract. No symbolic physical model or general theorem prover
is needed for these checks.

## One algorithm, one contract

The user's clarification requires two separate graph nodes:

| Node | Required input | Output |
| --- | --- | --- |
| `BayerDemosaic` | Bayer mosaic, with the sampling phase and value convention required by its algorithm | Dense camera RGB |
| `XTransDemosaic` | X-Trans mosaic, with the sampling information and value convention required by its algorithm | Dense camera RGB |

Connecting X-Trans data to `BayerDemosaic` is a contract error. The node does
not choose X-Trans demosaic instead. There is no generic demosaic node with a
mode-dependent contract or `Auto` processing node in this proposal. Missing
required sampling metadata leaves the selected computation incomplete; it
does not authorize inference or fallback inside the node.

Apply this structure to other distinct algorithms. Each node has one stable
rule set describing its concrete inputs, parameters, and outputs. Parameters
can supply different values within that rule set: a Bayer phase, matrix
coefficients, thresholds, or explicitly declared transform endpoints. These
do not require a different node for every parameter value. If multiple Bayer
demosaic algorithms are offered, give them explicit algorithm identities;
they can share the Bayer input contract without sharing an internal selector.

For example, a matrix-conversion operation always checks that its input
matches the matrix's declared source coordinates and emits the declared
target coordinates. Supplying another valid matrix does not change that
rule. A channel curve that does not consume colorimetric meaning can preserve
the supplied coordinate descriptor without branching over its identity.

Keep three boundaries distinct: a node defines an operation, reusable helpers
implement its arithmetic, and a UI groups operations for editing. Different
nodes may call the same helpers. A specified algorithm can contain several
stages or dispatches; “one thing” does not mean one arithmetic primitive per
node. Conversely, independent graph nodes still incur their normal dispatch
and buffer costs. Sharing source helpers does not fuse arbitrary graph nodes.

Minimize switches between unrelated algorithms and interpretations. Necessary
boundary handling, numerical guards, and data-dependent steps remain part of
the selected algorithm. Fewer branches alone is not a performance guarantee.
CPU/GPU placement remains an execution choice for the same declared operation,
not a reason to guess a different semantic algorithm.

## Initial checks

| Check | Example and justification |
| --- | --- |
| Graph structure and required inputs | A cycle contradicts the DAG definition. A missing mandatory input leaves the requested computation incomplete. |
| Concrete payload and sample roles | A CFA scalar raster cannot directly satisfy a dense three-channel raster input. An ordinary grayscale image does not acquire CFA meaning from its dimensions. |
| Coordinate identity and encoding used by the operation | A kernel specified to use linear Rec.2020 coefficients cannot treat declared camera responses or encoded sRGB samples as those coordinates. A defined conversion or intentional assignment of new meaning is a separate operation. |
| Explicit profile/parameter applicability | Applying a profile declared for camera A to declared camera B violates a camera-profile application contract. A generic matrix operation can have a weaker contract. |
| Required shape and sampling relationships | An elementwise operation without broadcasting/resampling needs matching shapes. A Bayer-specific implementation needs a supported pattern and its current phase. |
| Required numerical and memory-safety conditions | A normalizer must handle or reject a zero denominator. Indexing must respect actual extents. Enforce the selected implementation's conditions; do not impose blanket positivity or `[0,1]` restrictions on images. |

Unsupported execution backends, unavailable files, and memory exhaustion
remain operational failures. They do not add semantic rejection rules.

Only inspect geometry relationships that an operation actually needs. A crop
must update its CFA phase because that is executable sampling information.
The first version need not infer that a mask depicts the same photographed
subject, nor prove physical registration from image contents.

## Small amount of descriptive data

Retain concrete CPU/GPU payload schemas. Add a small, explicit vocabulary for
the meanings consumed by these operations, rather than an arbitrary set of
capability tags. Conceptual examples, not proposed Rust definitions:

```text
SensorMosaic(camera_coordinates, pattern, phase, value_convention)
CameraRGB(camera_coordinates, value_convention)
ColorRGB(space, encoding)
XYZ(reference_white, scale) / Lab(reference_white)
Scalar(role)

ColorTransform(input_interpretation, output_interpretation)
ColorLut(input_interpretation, output_interpretation, numeric_domain)
```

Dimensions and scalar storage formats belong to the concrete raster layout.
Grid mapping, scalar roles, scales, and alpha conventions are additional
fields only for operations that consume them. These examples are payload
families to introduce as needed, not a requirement to implement every color
space before the first node.

`space` resolves to a declared coordinate definition, including primaries and
white point for matrix RGB spaces. Compare coordinate definitions or canonical
identifiers, not display labels or profile filenames. `encoding` names the
mapping used to store those coordinates: identity, sRGB, or a supported log
encoding, for example.
Identity encoding does **not** assert that every preceding operation was
linear, or that the current image remains proportional to the original scene.
A creative tone curve can output newly edited, identity-encoded RGB values.

For raw preparation, the useful distinction is source code values versus
black-relative values with a declared normalization. The latter can be defined
by `(sample - black) / (white - black)`. It is not a claim about full-well
capacity and does not promise that every sample lies in `[0,1]`. Keep clipping
levels in the convention needed by a consuming reconstruction node; update
them when an operation changes that convention.

Camera-coordinate identity matches a profile's declared domain; it need not
be a camera serial number or a literal EXIF string. This checks declared
compatibility, not the accuracy of the profile or the history of white balance.

Keep the schema and interpretation logically separate even when one concrete
Rust type packages them together. The same three-channel storage can carry
camera responses or working RGB. A scalar mask and a scalar sensor mosaic
can share storage without becoming interchangeable. Interpretations belong
to individual values/ports, not mutable graph-wide state.

These are fields and identity checks, not a general algebra or a requirement
for a theorem prover. A node's concrete data layout remains known to its
implementation; metadata values may be determined at runtime. CPU/GPU
residency does not change color or sampling meaning.
The checker trusts descriptors established at input boundaries; it cannot
verify their physical truth from the pixels.

Do not require every node to prove a relationship to the original capture,
propagate a confidence score, or carry a noise covariance model. Optional
profiles and algorithm assumptions can be documented alongside their use.
They need not participate in graph compatibility unless a selected explicit
rule requires the supplied information.

Unknown optional physical information does not block processing. A missing
matrix, CFA pattern, or other value needed to execute a selected operation
still needs a declared default, an input, or a report that the configuration
is incomplete. Missing information is not evidence that a supplied physical
interpretation is false.

## Node declarations and propagation

A node declares its concrete port schemas, a small list of checks for its
specified algorithm, and how it constructs output metadata.
Each rule needs an understandable explanation of the mismatch it detects.
The initial rule vocabulary can be finite: kind membership, supported
pattern/encoding, equality of declared coordinates, and required relationships
between inputs. There is no need for user-defined logical propositions or
general refinement inference.

Examples, using conceptual names rather than a Rust API:

```text
Bayer demosaic:
  payload: concrete scalar float raster
  requires: SensorMosaic with Bayer pattern, known phase, required value convention
  produces: concrete RGB float raster, CameraRGB with declared response convention

X-Trans demosaic:
  payload: concrete scalar float raster
  requires: SensorMosaic with X-Trans pattern, known sampling, required value convention
  produces: concrete RGB float raster, CameraRGB with declared response convention

Apply camera color transform:
  payloads: concrete RGB float raster + concrete transform data
  requires: image coordinates and convention match transform's declared input
  produces: RGB in the transform's declared output space and encoding

Rec2020 luminance:
  payload: concrete RGB float raster
  requires: ColorRGB(Rec2020, identity)
  produces: concrete scalar float raster, Y in the input's scale, same grid

Generic RGB channel curve:
  payload: concrete RGB float raster
  requires: CameraRGB or ColorRGB; numerical behavior defined by the node
  produces: edited values in the same declared coordinates and encoding

Apply color LUT without input conversion:
  payloads: concrete RGB float raster + concrete LUT data
  requires: image interpretation matches LUT input interpretation
  produces: LUT output interpretation, same grid
```

Changing to another algorithm means selecting another node with its own
contract. Changing a parameter or input descriptor rechecks the affected
relations within the existing contract. Neither needs a general
dependent-type system or a mode switch in the contract checker.

Nodes must maintain the metadata that this small checker consumes. A crop
cannot retain the wrong phase; a color conversion cannot retain the old
coordinate definition. These are necessary local rules. General preservation
of calibration evidence or physical measurement fidelity is omitted.

At connection, reject known mismatches and defer requirements whose source
metadata is not yet available. At evaluation, check remaining shape, metadata,
and necessary numerical conditions once they are known. Unresolved required
metadata makes the selected computation incomplete; it is not proof of a
physical contradiction. Numerical guards and exceptional-case behavior belong
with the node.
For example, a LUT's numerical domain defines its lookup coordinates and
out-of-range policy; it need not require a proof that all pixels lie inside it.

Small host-side descriptors are sufficient for these semantic checks. Kernels
still receive concrete buffers and concrete parameters, with matrices or
levels supplied explicitly. This does not require general inference of value
ranges or downloading GPU images merely to characterize them. It imposes no
choice between WGSL and CubeCL.

## Proposed acceptance examples

These are design cases, not claims about how either application's existing
graph validator behaves and not an implemented test suite.

| Graph fragment | Decision under the proposed contracts |
| --- | --- |
| Normalized Bayer mosaic → demosaic → camera conversion → identity-encoded Rec.2020 → Rec.2020 luminance | Accept; each operation receives its declared meaning. |
| Demosaic → camera RGB → `filmcurv` colorimetric variant | Reject: camera coordinates are not Rec.2020 coordinates. |
| sRGB-encoded RGB → Rec.2020 luminance | Reject: both coordinates and encoding differ. |
| Black-relative normalized mosaic → apply original source black/white normalization | Reject for this specifically defined operation; the calibration's input convention no longer matches. |
| Demosaic → sensor highlight reconstruction | Reject: dense samples cannot satisfy a CFA reconstruction contract. |
| Camera RGB → generic channel curve | Accept; the operation does not claim colorimetric meaning for its coefficients. |
| Supported non-Rec.2020 working RGB → profile-aware color balance | Accept with the required matching profile data. |
| Working RGB → convert to LUT domain → LUT → convert from LUT output domain | Accept when each conversion exists and its declared domain matches. |
| A camera-log-to-working-RGB LUT → decode camera log again | Reject: the LUT output is already declared in a different encoding/domain. |
| Camera A image → profile explicitly declared for camera B | Reject the declared applicability mismatch; no assessment of profile accuracy is needed. |
| X-Trans mosaic → Bayer demosaic | Reject: the node requires Bayer samples. No substitution or bypass. |
| X-Trans mosaic → X-Trans demosaic | Accept when that algorithm's sampling and value conventions match. |
| Mosaic with missing CFA identity → either demosaic node | Required information is missing; neither node guesses the pattern. |
| Two graded branches in the same coordinates/encoding → same-coordinate blend | Accept, including intentionally encoded-space blending when that is the chosen operation. |
| Two curves in succession; display-targeted RGB → generic matrix/curve | Accept; no universal processing-stage restriction is defined. |

Examples of useful diagnostics: “expected Bayer mosaic; received X-Trans,”
“expected Rec.2020 with identity encoding; received sRGB with sRGB encoding,”
and “transform expects camera A coordinates; image declares camera B.” A bare
“not linear” or “invalid pipeline order” does not identify these errors.

## Deliberate limits

| Case | Initial behavior |
| --- | --- |
| Camera RGB into a Rec.2020-specific color operation | Reject the coordinate mismatch. |
| Camera RGB into a generic channel curve | Allow if its declared numerical requirements hold. |
| Bayer-only demosaic connected to declared X-Trans samples | Reject the unsupported sampling interpretation. |
| Two ordinary tone curves in succession | No general rejection. |
| Noise profile becomes a poor approximation after processing | No general model-validity tracking; ordinary parameter validation still applies. |
| Missing dark/flat calibration | No rejection solely for reduced physical accuracy. |
| Color calibration after sigmoid | No universal ordering rule. Reject only a mismatch covered by that specific node's declared, maintained input contract. Other unsuitable uses may pass. |
| Negative RGB or values above one | No universal rejection. The selected algorithm must define its handling or state a necessary domain check. |

In particular, a `scene/display` label must not become a shortcut for rejecting
all unusual ordering. Such a field can be useful descriptively. A hard rule
using it needs a specific contract and meaningful propagation rules; that
work is not avoided by naming the field. Consequently, this initial proposal
does **not** guarantee rejection of color calibration after sigmoid. It does
catch a mismatch between camera coordinates and a calibration implementation
that requires a defined working RGB space. A narrower scene-correction node
could add a reference contract later, separately from a general chromatic
adaptation or channel-mixing operation.

This is a scope choice, not a claim that reference roles are impossible to
check. A declared scene/output role can catch a narrower intended-use
mismatch if its producers and consumers have clear rules. It still would
not detect every calibration-invalidating edit within the scene side. Do not
reintroduce the first study's full capture-model tracking under a single
boolean label.

This is the remaining challenge to the practical direction: choose omissions
explicitly. Adding a convenient ordering blacklist to regain coverage would
give up the promise to reject only the selected clear contract violations.

## Current disposition

| Status | Item |
| --- | --- |
| Requirement direction | Prefer practical, incomplete error detection; omit algorithm nuances from the general type system. |
| Requirement | Minimize branching; each algorithm does one thing. Bayer and X-Trans demosaic are separate nodes, with no guessing inside a node. |
| Tentative | Start with the concrete checks and acceptance cases above; expand only for demonstrated error classes. |
| Tentative | Keep physical model quality and processing-order advice out of hard rejection. |
| Tentative | Use per-output interpretations, explicit transform endpoints, and fixed algorithm contracts instead of inheriting upstream metadata and fallback conventions. |
| Open | Whether the proposed examples cover enough ordinary errors, especially whether a distinct scene-correction contract is needed. |
| Open | Exact payload schemas, CPU/GPU transport, and backend selection remain undecided. |

This follow-up updates only the separate redesign documents. No implementation
or changes to the original project logs are part of this session.
The cited source paths were inspected at the recorded revisions. The
enforcement proposal and upstream alternatives are not implemented or
empirically established to catch most user mistakes yet.

## References

[1] darktable developers, “darktable source code,” revision
`733bd69f32cac7ff5e41025115942772add1f088`, accessed Oct. 8, 2026.
[Online]. Available: [pinned source tree](https://github.com/darktable-org/darktable/tree/733bd69f32cac7ff5e41025115942772add1f088).
Individual files and locations are linked beside the observations above.

[2] J. Hanika and contributors, “vkdt source code and module documentation,”
version 1.0.0, revision `afcc804898715d05952066718f4e4c0eba21ed76`, accessed
Oct. 8, 2026. [Online]. Available:
[pinned source tree](https://github.com/hanatos/vkdt/tree/afcc804898715d05952066718f4e4c0eba21ed76).
No computational code or algorithms were ported in this study.
