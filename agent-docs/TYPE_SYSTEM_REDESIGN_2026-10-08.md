# Type-system redesign: first-principles study

Date: 2026-10-08. Status: **proposal**, pending discussion.

**Scope update, 2026-10-08:** the user subsequently proposed catching a
practical subset of logical errors and excluding algorithm nuances. They also
require separate Bayer and X-Trans demosaic nodes, without guessing inside a
node, and minimizing branching between distinct algorithms. The
[practical proposal](TYPE_SYSTEM_REDESIGN_PRACTICAL.md) is the current proposed
enforcement scope. The broader model/evidence machinery below remains
exploratory background, not a requirement for the first design.

The user's statements are recorded separately in
[TYPE_SYSTEM_REDESIGN_USER_NOTES.md](TYPE_SYSTEM_REDESIGN_USER_NOTES.md).
Existing Drip implementation and design documents are not premises of this
study. No implementation is proposed here as already agreed.

The working proposal is concrete payload schemas with explicit interpretations,
relations between values, and algorithm-specific contracts. Kernels receive
buffers, numbers, matrices, and tables. The contract checker reasons about
those values outside the pixel loop. Neither a universal algebraic object nor
a hierarchy of pipeline stages is necessary.

## 1. Challenges to the prompt

### “Reject iff logically wrong” needs a defined scope

There are at least four different questions:

1. Is this operation defined on these representations and numerical values?
2. Do the supplied coordinates, units, and reference data meet its contract?
3. Is its model a good approximation to this particular photograph?
4. Is the result desirable?

The first two admit useful checks. The third often involves unobserved facts;
the fourth belongs to the user. An image cannot prove that its illuminant was
uniform, that a surface was neutral, or that a camera profile fits its spectra.
Different physical situations can produce the same recorded samples.

A complete automatic classifier for arbitrary node programs and arbitrary
semantic predicates is also not available. Restricting the contract language
makes particular questions decidable; it does not make physical truth
observable. Abstract interpretation provides sound, potentially incomplete
information about computations [8].

This does not prevent exact checks for a finite library of explicitly
specified node contracts. It limits what those checks establish: consistency
with those contracts, rather than every possible physical or creative meaning.

**Proposal:** reject demonstrated contract contradictions; defer checkable
unknowns; require values for missing operational inputs; permit explicit,
qualified modeling assumptions. Do not label an unresolved contract as false.
Out-of-memory, missing files, and device loss are execution failures, not
proofs that a graph is logically invalid. This is a qualification of the
literal “iff,” not a claim to have achieved it.

### Camera RGB does not make all grading undefined

Three sensor-response channels support channel gains, mixing, and scalar
curves. A curve does not require CIE colorimetry simply to act on numbers.
What is unavailable without additional information is a justified use of
Rec.2020 luminance weights, perceptual hue coordinates, or a renderer whose
primaries have specified colorimetric meaning.

Consequently, distinguish a generic channel curve from a specified color
grading algorithm. Reject applying Rec.2020 coefficients to a value still
declared as camera responses. Do not reject every creative operation on
camera responses. An explicit reinterpretation can deliberately assign
Rec.2020 meaning to the numbers, but cannot preserve a claim that it calibrated
the camera.

### “Color calibration” is not one mathematical contract

Darktable combines adaptation, mixing, and calibration in one module. Its
warnings explicitly accommodate valid exceptions to the usual workflow [D1].
Sensor-space calibration is possible before an input transform. Adaptation of
rendered colors is possible after a display transform. Neither is automatically
the same operation as correcting the original scene illuminant.

For a particular capture-calibration model, applying its unchanged parameters
after a nonlinear rendering is generally unjustified. That is a mismatch
between the model and its input, not a universal ban on calibration after
sigmoid. Section 2 makes this distinction algebraically.

### Reference state and encoding are independent

Scene-referred does not mean linearly encoded. CIE defines it in terms of
estimated scene color coordinates, while output-referred data concerns a
rendered image and specified output/viewing conditions [1], [2]. ACEScct is a
logarithmic encoding used for scene-referred grading [3]. Darktable's sigmoid
documentation describes nonlinear processing producing linear RGB,
display-referred output [D2].

Also distinguish the reference white of RGB coordinates from the scene's
estimated illuminant and the observer's adopted white. Sharing “D65” in a
description does not make these interchangeable pieces of metadata.

### Noise does not invalidate a useful response model

Quantum efficiency changes the response to incident photons; shot/read noise
makes observations uncertain. Neither alone prevents a linear model for the
*mean* response in an unsaturated operating region. However, normalized raw
codes are not generally measured fractions of full-well capacity. EMVA
explicitly distinguishes digital saturation capacity from full-well capacity
[4]. Missing detailed calibration can reduce confidence without making the
representation or the operation undefined.

### Concrete kernels do not require compile-time colorimetry

A matrix's layout and scalar type can be known at compile time while its
coefficients, camera identity, image dimensions, and adopted white are known
at evaluation time. Concrete schemas are sufficient for this separation.
GPU execution does not by itself rule out useful mathematical abstractions;
it rules out leaving a kernel's representation and executable operations
unspecified. Even floating-point arithmetic does not satisfy all laws of an
exact real vector space.

### A CPU/GPU bijection needs a domain of equivalence

An RGB CPU array and a padded GPU array need not be in bijection as arbitrary
byte strings. They can represent the same logical sample sequence. Require
lossless transport between supported *logical values*, using canonical
padding or ignoring padding in equality. An `f32` to `f16` conversion is a
precision-changing computation, not such a transport.

Exact transport also does not imply bit-identical CPU/GPU kernel results.
WGSL permits numerical behavior that must be considered explicitly [10].
Choose either a numerical equivalence contract for backend implementations,
or a stricter determinism requirement with its costs; do not conflate either
with memory-layout conversion.

### Purity and memory management need explicit boundaries

A path is not immutable file contents. Reading the same path twice can return
different bytes. Source identity must refer to a frozen asset version, or
loading must precede pure decoding. Profiles, random seeds, frame identity,
and semantic context must likewise be fixed for an evaluation.

Memoization can preserve this value semantics, but hidden, unbounded caches
conflict with the memory objective. Absence of a persistent DAG cache does
not eliminate live intermediates, branch retention, scratch space, or queued
GPU work. A 100-megapixel four-channel `f32` image occupies about 1.49 GiB;
three such buffers already exceed 4 GiB. A memory policy is needed even for
the first evaluator.

## 2. Mathematical distinctions the contracts must express

### 2.1 Observation, normalization, and approximation

A useful simplified observation model is

```text
N_p ~ Poisson(mu_p)
mu_p = integral eta_p(lambda) Phi_p(lambda, t) d(lambda) dt
d_p = quantize_and_clip(b_p + g_p (N_p + noise_p))
```

Here `Phi` is incident photon rate per wavelength at a photosite, `eta`
includes its spectral response, `g` converts electron counts to codes, and
`b` includes a modeled offset. Dark signal and other disturbances require
further terms or approximations. This is an illustrative model, not a claim
that every raw file satisfies it [4], [7].

A normalization operation can make the much narrower statement

```text
r_p = (linearize(d_p) - B_p) / (W_p - B_p),  with W_p > B_p.
```

`B` and `W` must be in the same linearized code coordinates. This defines
zero and a clipping reference without claiming electron counts, incident
photon counts, exact radiance, or a full-well fraction. It does not imply
that all samples lie in `[0,1]`; noise, estimated levels, and later gains can
produce values outside that interval.

**Minimum useful raw interpretation:** samples of named sensor responses on
a stated sampling lattice, with a stated response encoding, offset/scale
convention, and clipping information where an algorithm needs it. A response
model may be supplied by metadata, calibration, estimation, or a declared
default. Its physical adequacy remains an assumption.

### 2.2 Sensor responses are not automatically tristimulus coordinates

Discretize a spectrum as `s`. Write camera responses as `q = Q s` and a
specified observer's tristimulus coordinates as `x = C s`. A universal exact
matrix conversion requires

```text
C = M Q.
```

This is the relevant colorimetric condition, rather than both arrays having
three components. If there is a spectral perturbation `z` with `Q z = 0`
but `C z != 0`, the camera cannot distinguish spectra that the observer
distinguishes. No later type annotation recovers that information. The
Luther-condition literature makes the exact/approximate distinction explicit
[5].

A camera profile supplies a conversion model, commonly an approximation over
an intended domain. Its output can have well-defined XYZ or working-RGB
coordinates while remaining an estimate of the scene's colorimetry. A type
system need not certify a perfect camera to permit that operation. ACES input
transforms are a concrete example of a specified camera-to-working-encoding
operation [6].

### 2.3 Encoding, editing, and capture relationships

Write stored color values as `v = E(c)`, where `c` is a color-coordinate vector
and `E` is its encoding. A nonlinear creative edit `c_new = G(c)` can produce
new coordinates stored with the same identity encoding. It need not change
the storage interpretation to `E = G`.

What may cease to hold is a relation such as `c ≈ M q` for the original
capture `q`, or proportionality to its estimated light. Preserve the current
coordinate definition; preserve a capture model only when the edit supplies
an applicable transformation of that model. “Scene-referred” in a creative
workflow is not a certificate of unmodified measurements.

For a full-adaptation von Kries-style transform in XYZ coordinates, one
illustrative formula is

```text
A = H^-1 diag((H W_destination) / (H W_source)) H
x_out = A x_in
```

Here the whites use a common luminance normalization. It needs defined
coordinate/white conventions, an invertible `H`, and valid denominators.
Specific models add adaptation and viewing parameters. CATs
predict corresponding appearance under their models; they are not general
spectral relighting operators [12], [13].

If an invertible camera conversion model is `x = M q`, the corresponding
camera-coordinate operation is `M^-1 A M`. Thus “before camera conversion”
is not inherently meaningless. For a nonlinear rendering `F`, in general
`A F(x) != F(A x)`. Transporting the *same* correction through an invertible
`F` would require `F A F^-1`, not merely applying `A` to its output. Clipping
or other information loss can prevent that inverse. A new adaptation of the
rendered image remains a different, definable operation.

### 2.4 Models and auxiliary values have coordinates too

For a scalar noise model `Var(x | mu) = a mu + b`, positive scaling `y = k x`
gives

```text
Var(y | mu_y) = (k a) mu_y + k^2 b.
```

Copying the old coefficients is wrong. Mixing channels transforms covariance
as `Sigma_y = M Sigma_x M^T`; even independent input channels can become
correlated. Resampling can introduce spatial correlation. These consequences
follow from covariance algebra, independently of how good the initial noise
model was [7].

Likewise, clipping thresholds must follow gains, a crop must update CFA phase,
and an image mask must follow its image's geometry. A pixel value alone does
not reveal whether it was clipped before later processing. Preserve evidence
needed by a consumer, or withdraw the corresponding guarantee.

## 3. A small vocabulary, with explicit relations

Status: **tentative**. These are conceptual records, not Rust trait designs.
Only introduce a field when an actual contract consumes it.

| Component | Information that earns its place |
| --- | --- |
| Concrete payload schema | Scalar format, channel/plane structure, shape, layout variants, and any associated buffers or tables. Examples: scalar raster, three-channel raster, mask, matrix, profile table. |
| Sampling and channel meaning | Coordinate domain and grid mapping; CFA pattern and phase where applicable; channel identities; straight/premultiplied alpha when present. Equal dimensions do not establish registration. |
| Value coordinates | Sensor-response identity and encoding; or color-coordinate system, observer, encoding, white convention, and scale; or another quantity such as dimensionless weight or optical density. |
| Reference and applicability | Which capture, current image, output target, or calibration domain a fact describes. Distinguish capture relations from current authored color coordinates. |
| Required models and evidence | Camera transform, neutral estimate, clipping mask, noise model, PSF, LUT definition, and their applicability to particular values. Large data are ordinary concrete ports; small definitions can be parameters or immutable context. |
| Numerical predicates | Actual requirements such as finite denominators, positive log arguments, representable sizes, and algorithm-specific ranges. These are not universal image restrictions. |

Residency, device identity, readiness, and permitted physical access belong
to execution planning. They cannot answer whether samples mean camera
responses or Rec.2020. Small host metadata should remain available without
downloading a device-resident image.

Avoid global tags such as `Linear`, `Calibrated`, `HasWhiteBalance`, or
`ValidColor`. Each hides essential arguments. Prefer claims of the form
“this profile consumes these sensor coordinates,” “this neutral estimate
applies to this capture after these gains,” or “this mask is registered to
this grid.” The graph already supplies history; contracts need current
applicability and relevant dependencies, not a duplicate unbounded history.

Distinguish three kinds of statement:

- **Definition:** these numbers are interpreted as linear Rec.2020 coordinates
  with a specified scale. This assigns meaning; it does not prove scene
  accuracy.
- **Checkable fact:** a shape matches, an identifier agrees, all required
  denominators are nonzero, or a range certificate follows from a computation.
- **Modeling assumption:** a camera response is adequately linear in a stated
  region, an estimated illuminant is appropriate, or a noise model is useful.

An assumption records its source and scope. It is not promoted to a measured
fact. A model may have been calibrated experimentally and still be an
approximation; a single confidence number is not an adequate substitute for
its applicability conditions.

## 4. What a node requests and promises

For fixed context and parameters, specify a partial function over concrete
values:

```text
N(context, parameters, input ports) -> output ports

input schemas
required coordinate and cross-port relations
numerical domain, including defined handling of exceptional cases
modeling assumptions used by the interpretation of the result
output schemas and interpretation/fact transfer rules
```

The distinction between a mathematical operation and a physical estimator
must be in that specification. A generic matrix multiplication only needs
conformable values. A camera-calibration application additionally needs a
model whose input coordinates match. They may share one computational kernel.

“Capability” should mean available, applicable information that supports a
specific operation. For example, computing CIE `Y` requires a usable mapping
from the current coordinates to the specified XYZ coordinates; applying a
calibration requires the calibration payload and its input relation. A marker
called `Colorimetric` supplies neither on its own. Preparation lowers this
information to a concrete matrix, table, or selected kernel variant.

Many requirements are relational or parameter-dependent. A noise profile
must describe *this* input in *these* units. A selected Bayer demosaicer needs
a compatible CFA. A channel-mixer mode must not inherit the stricter
requirements of a disabled CAT mode. Defaults are actual declared choices
with consequences, rather than hidden missing inputs.

This resembles refinements over concrete types, with relations to other
values [9]. It does not require implementing a general refinement-type
language, arbitrary theorem prover, or algebraic trait hierarchy.

More formally, let `S` describe the possible inputs allowed by upstream
contracts and known metadata, and let `R` be this operation's required domain:

```text
S is contained in R       => compatible
S and R are disjoint     => incompatible
otherwise                => conditional; information or a runtime check is needed
```

Cross-port relations are predicates over the input tuple. A proposed initial
checker only needs explicit schema cases, identity/equality constraints,
supported coordinate transforms, and a small set of numerical predicates.
More expressive predicates need their own validation strategy; they must not
silently become successful checks when the checker cannot decide them.

An output definition follows from the computation's specification. An output
claim about the physical scene follows only conditionally on the applicable
model assumptions. Compositional correctness also depends on node authors
implementing their preconditions and transfer rules correctly. A false
producer guarantee remains a bug; attaching a contract does not prove the
kernel satisfies it.

### Connecting and evaluating

1. **At connection:** check schemas, known coordinate incompatibilities,
   cross-port constraints, parameter-selected requirements, and acyclicity.
   Carry unresolved dimensions/profile facts as explicit obligations.
2. **Before evaluation:** freeze the context and parameters for the requested
   dependency subgraph. Resolve asset metadata, models, shapes, and the
   execution plan. Recheck obligations affected by edits.
3. **At data boundaries:** establish remaining numerical facts from trusted
   producer guarantees or validation on the resident backend. A failed
   predicate reports the specific requirement and value involved.
4. **After an operation:** derive the output interpretation and guarantees.
   Preserve a claim only through a stated rule. Withdraw unsupported claims;
   do not retain metadata simply because the storage shape is unchanged.

Connection results should distinguish **compatible**, **incompatible**, and
**conditional**. Evaluation can separately report **missing information**,
**failed data precondition**, or **execution failure**. Unknown physical
accuracy is not a numerical precondition to scan for.

A valid connection does not promise that every possible future input image
will satisfy a data-dependent precondition. Conversely, inability to prove
that all samples are positive statically does not justify rejecting a
connection whose actual input can be checked.

## 5. Module survey and candidate minimum contracts

Darktable references below are its development manual as consulted on
2026-10-08. vkdt references are its `1.0.0` source at commit
`afcc804898715d05952066718f4e4c0eba21ed76` [V1]. The table compares operation
families, not exact equivalent implementations. Requirements in the last two
columns are **our proposed analysis**, not claims that either application
enforces them. Exact numerical domains require examination of the chosen
algorithm and version before implementation.

| Operation family and observed modules | Concrete inputs and minimum contract | Assumptions and output consequences |
| --- | --- | --- |
| Raw import: vkdt `i-raw` [V2] | Asset snapshot and decoder configuration; output scalar CFA samples or the decoded camera-channel schema, plus applicable metadata. | Decoding does not certify photon counts, a linear response, or a complete camera calibration. |
| Black subtraction and normalization: darktable raw black/white point [D3]; part of vkdt `denoise` [V3] | Scalar/camera raster; offset and white reference in matching coordinates; linearization if the declared algorithm needs it; `W > B`; applicable active area. | Produces the specified normalized response estimate. `[0,1]` is a reference convention, not an automatic range guarantee. |
| Flat-field/gain-map correction: darktable raw black/white point [D3]; vkdt `denoise` [V3] | Response raster and registered correction field; matching channel and scale conventions; valid division or gain values. | Assumes the calibration is applicable to this optical/sensor configuration. Corrected response is still an estimate; clipping/noise evidence needs corresponding treatment. |
| Hot-pixel filtering: vkdt `hotpx` [V4] | Scalar CFA raster, supported pattern and phase, neighborhood/border rules, threshold in its declared units. | Outlier and local-similarity assumptions can fail without making the operation undefined. Replaced samples are estimates. |
| Demosaic: darktable demosaic [D4]; vkdt `demosaic` [V5] | Scalar raster, channel-sampling map including phase, supported pattern, required neighborhood and response convention for the selected algorithm. | Needs a spatial/color correlation prior, not exact QE or full-well capacity. Produces dense camera-channel estimates, not automatically observer colorimetry. |
| Sensor channel gains / as-shot WB: darktable white balance [D5]; gains in vkdt `colour` [V6] | CFA or dense camera raster; applicable channel gains or a neutral estimate from which to derive them; correct existing gain convention. | Physical neutralization assumes a usable neutral estimate. Update gains, noise scaling, and clipping coordinates; do not claim exact relighting. |
| Sensor highlight reconstruction: darktable highlight reconstruction [D6]; vkdt `hilite` [V7] | Supported sensor layout; clipping evidence or specified thresholds in current response coordinates; neighborhood rules; defined fallback when evidence is exhausted. | Spatial/chromatic continuity is a prior. Filled values are estimates, not recovered measurements. All channels clipped is an inference limitation, not necessarily an execution error. |
| Profiled denoising: darktable denoise (profiled) [D7]; vkdt `denoise` [V3] | Raster plus noise model or chosen noise parameters applicable to its channel basis, scale, and processing domain; valid model parameters for the selected method. | Camera/ISO matching is evidence, not proof of the full model. Demosaicing and resampling may change correlations. Withdraw an unchanged raw-noise guarantee after filtering. |
| Camera input transform: darktable input color profile [D8]; vkdt `colour` [V6] | Camera raster and matrix/profile whose input response, normalization, gains, and calibration conditions match; explicit target coordinates. | Produces a colorimetric estimate under that model. The output's coordinate definition can be exact while its relation to scene color is approximate. |
| CAT / color calibration: darktable color calibration [D1]; portions of vkdt `colour` [V6] | Current color coordinates, usable mapping to the CAT coordinates, source/destination white and required viewing parameters. Capture-calibration mode additionally needs its capture relation. | Predicts adaptation under a model. Generic mixing and creative mapping require weaker contracts. Update the adopted-white relation; do not infer a universal “WB done” flag. |
| Exposure scaling: darktable exposure [D9]; vkdt `exposure` [V8] | Numeric raster and finite scale within the chosen numerical domain. Calling the result an exposure change requires linear coordinates for the quantity being scaled. | Can scale sensor responses, scene estimates, or intended display light. Does not undo sensor clipping; update scale-dependent models. |
| Creative grading / curves: darktable color balance RGB [D10]; vkdt `grade`, `curves` [V9], [V10] | Per-channel modes need specified channels and curve domains. Luminance/perceptual modes additionally need their color conversion, white/scale conventions, and exceptional-value policy. | These modes do not have one universal input contract. Preserve the declared output coordinates; retain original-capture relations only if justified. vkdt documents different reference-state expectations for `grade` and `curves`. |
| Display rendering: darktable sigmoid and filmic RGB [D2], [D11]; vkdt `filmcurv` [V11] | Color raster in the renderer's stated coordinates and encoding, exposure/gray normalization, intended output conditions, and algorithm-specific handling of negative/extreme values. | Produces rendered color coordinates. The capture-light relation generally changes. Output may remain linear encoded; rendering and output encoding are separate operations. |
| Local tonal adjustment: darktable tone equalizer [D12]; vkdt `llap` [V12] | Raster, selected intensity/luminance estimator and scale, spatial neighborhood, valid log/ratio domains or declared extensions. | A norm used as an intensity proxy is not automatically CIE luminance. Spatially varying adjustments can break a global capture-response model without destroying color coordinates. |
| Crop, resampling, lens warp: darktable lens correction [D13]; vkdt `crop`, `lens` [V13] | Sampled image and coordinate map, sampling kernel, boundary policy; model parameters if claiming lens correction. | Update grid and auxiliary-data registration. CFA crops change phase; arbitrary interpolation across CFA colors does not preserve a CFA interpretation. Physical reconstruction quality is an assumption. |
| Masks and compositing: vkdt `mask`, `blend` [V14] | Registered scalar masks and rasters. Color compositing needs compatible color coordinates, reference scales, and alpha conventions; raw numeric blending may intentionally use weaker semantics. | Same dimensions alone are insufficient. Do not preserve a single capture-calibration relation through an arbitrary composite. Unusual blends can still be defined. |
| LUT application: darktable LUT 3D [D14]; vkdt LUT inputs and profile use [V6], [V15] | Raster and concrete table with declared input/output coordinate encodings, sampled domain, interpolation, and out-of-domain policy. | Table entries alone do not identify whether the LUT is calibration, a look, rendering, or encoding. Its output claims come from its declared definition. |
| Deconvolution: vkdt `deconv` [V16] | Raster in the signal coordinates of the blur model; PSF/operator with applicable sampling; algorithm-specific positivity and denominator handling. | Optical interpretation assumes an appropriate blur/noise model. “After demosaic” is a recommendation, not a sufficient specification of these conditions. |
| Film-negative inversion: darktable negadoctor [D15]; vkdt `negative` [V17] | Scan/capture in the model's response coordinates; base/reference measurements and film model parameters. A density calculation needs a positive, dimensionless transmittance ratio or an explicit floor. | Normalized transmission and estimated original-scene exposure are different quantities. Trichromatic film/scanner models are approximate. Output meaning follows the chosen inversion/rendering model. |
| Dehazing: vkdt `dehaze` [V18] | For physical inversion of `I = t J + (1-t) A`, the quantities must share linear radiometric coordinates; airlight, transmission estimate, and a defined minimum transmission are required or estimated internally. | The dark-channel and atmospheric assumptions are not universal facts [14]. A transmission or optical-depth estimate does not become distance in meters without additional scale information. |
| Output conversion: darktable output color profile; vkdt `colenc` [V19] | Defined input color coordinates/encoding and specified output transform, white convention, scale, and numerical domain. | Distinguish color conversion, rendering, encoding, and quantization. Decoding a rendered JPEG's transfer function does not recover its original scene measurements. |

Two especially useful counterexamples are in the applications themselves:
vkdt `denoise` combines normalization with noise reduction [V3], while
darktable LUT 3D documents a camera-log use case outside its usual position
[D14]. A module name or recommended order is therefore a poor substitute for
the selected operation's contract.

## 6. Worked contract sketches

These sketches describe requirements, not a proposed DSL.

**Bayer demosaic.** Request a concrete scalar raster, a CFA definition whose
method is supported, and a phase that maps local coordinates to sensor
channels. Request only the normalization/gain conditions the selected
algorithm actually uses. Produce a concrete dense camera raster, retaining
sensor identity and stating the interpolation model. A generic grayscale
array is insufficiently described; an explicitly declared CFA sampling can
make synthetic data meaningful too.

**CIE luminance extraction.** Request a concrete color raster and an
applicable conversion to the selected observer's XYZ coordinates, including
encoding and scale. Lower that conversion to supported concrete operations.
The output is `Y` in that scale. A simple Rec.2020 dot-product implementation
requests linear Rec.2020 specifically; it cannot accept arbitrary camera RGB
merely because both have three channels. A camera profile can supply a
separate approximate conversion first.

**Capture-calibration application.** Request a concrete raster, a calibration
model, and a relation establishing that the raster is in the model's input
coordinates. Where the model relies on an original neutral estimate, request
its applicable capture/gain relation. A rendered image carrying the same
shape and primaries is insufficient. A creative matrix node can still accept
that image under a different specification.

**Profiled denoising.** Request the raster and a specified noise model in its
present units and basis. Scaling can transform the model analytically.
Following an arbitrary nonlinear edit, the original coefficients are no
longer justified by that rule. Supply a new/estimated model, choose a
declared approximation, or use a denoiser whose contract does not depend on
the old profile. Do not quietly keep the profile valid.

**Masked blend.** Request concrete input rasters and a scalar weight field,
an explicit grid correspondence, and the compatibility conditions of the
selected blend semantics. An image and mask with equal dimensions but
different crop origins do not meet a registration contract. If the user
chooses numeric index-wise blending, that defines a different operation and
must state the resulting interpretation.

## 7. Propagation and reinterpretation

An operation must specify what it preserves, transforms, creates, and can no
longer guarantee. Useful initial rules include:

- Lossless upload/download preserves logical samples and semantic evidence.
- A pure coordinate conversion preserves the represented color and reference
  state within its numerical contract; its inverse need not be available.
- Channel gains transform the applicable response model, clipping reference,
  and noise parameters. A gain is not proof of neutralization.
- A nonlinear creative edit can preserve current color coordinates while
  withdrawing the simple relation to the original capture.
- Cropping transforms the grid, CFA phase, and registered auxiliary values.
- Reconstruction changes the status of affected samples from observations
  to estimates. A consumer may still accept both.
- A composite retains only relations justified for the combination. Pointwise
  masks can make applicability spatially varying; do not assign a global
  calibration guarantee from a local correction.

Avoid a single `pristine` flag. That would reject valid operations after
benign changes and still fail to describe the relationship a calibration
actually needs. Conversely, do not attempt to preserve an arbitrary symbolic
inverse of the entire editing history. Implement a few useful model-transfer
rules and explicitly withdraw claims outside that supported set.

There are two different deliberate declarations:

1. **Assign meaning:** interpret an untagged array as a CFA or specified RGB
   coordinates. This may change the intended subject and withdraw incompatible
   claims. It does not convert samples.
2. **Assume a model:** accept a stated approximate response/noise/illumination
   model for a defined use. Derived physical claims remain conditional on it.

Neither declaration can bypass mismatched shapes, invalid denominators, or
missing kernel inputs. Assigning new coordinates may make a profile
computationally applicable, but cannot establish that it fits the original
capture. Converting values, assigning meaning, and asserting an approximate
physical model need distinct semantics. Silent reinterpretation at connection
would defeat the checks.

## 8. Cases the proposal must handle

| Case | Expected result and reason |
| --- | --- |
| Camera RGB into a Rec.2020-specific luminance kernel | Incompatible unless a defined conversion or deliberate reassignment establishes the required coordinates. |
| Camera RGB into a generic per-channel curve | Compatible if its numerical domain is met; output capture-model guarantees follow the curve's specification. |
| Log-encoded scene data into a linear-coordinate exposure kernel | Require decoding or a kernel implementing the equivalent encoded-domain operation. Scene reference alone is insufficient. |
| Sigmoid output into a display-color adjustment | May be compatible. Nonlinear processing does not imply nonlinear output encoding. |
| Sigmoid output into unchanged original-capture calibration | Reject the incompatible capture relation, unless an applicable transformed model is supplied. |
| Raw data without dark/flat calibration into demosaic | Permit with a stated response convention and adequate algorithm inputs; missing high-accuracy calibration alone is not a contradiction. |
| Negative samples after black subtraction or RGB conversion | Permit whenever the selected algorithm defines them. Negative coordinates alone do not prove physically impossible color. |
| X-Trans sampling into a Bayer-only kernel | Incompatible with this implementation. Another algorithm may support the same logical operation. |
| Odd-offset crop retaining the old CFA phase | Incorrect output contract; subsequent CFA processing would use the wrong channels. |
| Gain changed but clipping/noise model copied unchanged | Incorrect transfer rule unless those values are explicitly expressed in an unchanged reference coordinate system. |
| Finite inputs to an overflowing expression | Input finiteness is insufficient. The numerical contract needs bounds, a safe implementation, or a reported data-domain failure. |
| Two tone curves or two CATs in succession | No universal rejection. Compose their actual domains and reference transformations. Quality/order guidance is separate. |
| All highlights clipped | Execute a defined estimator/fallback when available. Do not claim recovered measurements; reject only an unmet requirement of a method without a fallback. |
| CPU and GPU representations of the same logical image | Semantically compatible; transport, device compatibility, and readiness are planning obligations. |
| Correct graph exceeding the device memory budget | Execution/resource failure, not a semantic type error. |

## 9. Consequences for hybrid evaluation

Status: **tentative**; neither WGSL nor CubeCL is selected.

The value-level node function remains pure. Its implementation can allocate
scratch buffers and issue multiple kernels. A “node” need not be one dispatch.
Execution effects belong to the evaluator/runtime; export I/O belongs to a
consumer of the evaluated result. If exact output bits are part of purity,
backend and numerical execution choices must also be specified. Mathematical
purity with documented implementation error is a different, weaker promise.

Evaluate the requested dependency subgraph. Track each logical value's
residency and outstanding readers. Keep intermediate values on the GPU,
transfer at necessary CPU/GPU boundaries, and release or reuse storage after
its last consumer has actually finished. In-place reuse is compatible with
pure value semantics only when no observable alias can see the mutation.

The budget must account for live branch inputs, outputs, scratch, staging,
retained source decodes, and work still in flight. An internal source cache
must participate in that budget. Track host and device limits appropriately
for discrete or shared-memory hardware. Decide later whether memory pressure invokes
recomputation, spilling, tiling, or a resource error; some global algorithms
cannot be tiled with a simple halo. Independent dispatch is an optimization,
not a promise of hardware overlap, and can raise peak memory use.

Most semantic checks operate on small metadata. Actual sample predicates may
need a GPU reduction and a small status readback, or a guarded execution plan.
A small readback still introduces synchronization. Prefer producer guarantees
and defined numerical extensions where appropriate; do not download full
images just to validate them. An extension such as flooring a log argument
must be part of the algorithm, not a hidden change made by the checker.

The next backend comparison should exercise actual requirements: one
pointwise color transform, one neighborhood kernel, one reduction with a
validation result, a table/profile lookup, and a CPU decode followed by a
resident GPU chain. Include compatible CPU execution of the same kernel
source, buffer reuse, and GUI access to GPU results. CubeCL advertises a
multi-backend compilation architecture [11]; that is a candidate to test,
not evidence that these particular needs already work satisfactorily.

## 10. Open choices and session record

| Status | Question or proposal | Reason |
| --- | --- | --- |
| Requirement | Concrete payloads, pure node semantics, GPU-resident processing, demand-driven DAG evaluation. | User constraints; exact equivalence/purity qualifications above still need discussion. |
| Tentative | Use local refinements and relations over concrete payloads. | Handles profiles, masks, CFA phase, and parameter-selected modes without abstract kernel inputs. |
| Tentative | Use a small set of declared model-transfer rules; withdraw unsupported guarantees. | Avoids both stale metadata and a symbolic reconstruction of the entire pipeline. |
| Open | How should the product present missing information versus contradiction? | A binary “valid/invalid” result conflates uncertainty with a demonstrated error. |
| Open | Which approximate models may imports establish by default, and which require an explicit user choice? | Ordinary raw processing must work without full calibration while keeping assumptions honest. |
| Open | How freely may users assign a new meaning to already interpreted data? | Broad creative freedom is consistent only if incompatible old guarantees are withdrawn. |
| Open | Which color-calibration operations are intended? | Capture correction, CAT, profile fitting, and creative mixing have different contracts. |
| Open | Required numerical equivalence across backends. | Exact transfer, approximate computation, and bitwise determinism are separate properties. |
| Open | Memory behavior under a roughly 4 GB budget. | No persistent result cache does not bound the live working set. |
| Open | WGSL versus CubeCL and CPU execution strategy. | Decide using concrete kernels and interoperability tests after contract agreement. |

This session established a separate user record, reviewed primary color and
sensor references, inspected vkdt's pinned module documentation, and compared
the operation families above with darktable's manual. It produced this
proposal and adversarial examples. It did not validate Drip's old type system,
port an algorithm, select a backend, or establish full contracts for every
algorithm variant. Existing project logs and implementation files were not
edited. No acceptance of the tentative design is inferred.

## References

Primary sources are used for reported behavior and scientific definitions.
Proposed contracts and their consequences are the analysis of this study.
Web sources were consulted on Oct. 8, 2026. No third-party code was adapted;
implementation-specific attribution belongs in `THIRD_PARTY.md` if a later
implementation uses it.

[1] CIE, “Scene-referred image state,” *International Lighting Vocabulary*,
    CIE S 017:2020, term 17-32-054, 2020.
    [Online](https://cie.co.at/eilvterm/17-32-054).

[2] CIE, “Output-referred image state,” *International Lighting Vocabulary*,
    CIE S 017:2020, term 17-32-052, 2020.
    [Online](https://cie.co.at/eilvterm/17-32-052).

[3] Academy of Motion Picture Arts and Sciences, “ACES system,” *ACES
    Documentation*, Sep. 10, 2025.
    [Online](https://docs.acescentral.com/background/overview/).

[4] European Machine Vision Association, *Standard for Characterization of
    Image Sensors and Cameras*, EMVA 1288, Release 4.0 Linear, Jun. 16, 2021,
    secs. 2–3.
    [PDF](https://www.emva.org/wp-content/uploads/EMVA1288Linear_4.0Release.pdf).

[5] G. D. Finlayson and Y. Zhu, “Designing color filters that make cameras
    more colorimetric,” *IEEE Trans. Image Process.*, vol. 30, pp. 853–867,
    2021, doi: 10.1109/TIP.2020.3038523.
    [Author repository](https://ueaeprints.uea.ac.uk/id/eprint/78083/).

[6] Academy of Motion Picture Arts and Sciences, “Input transforms,” *ACES
    Documentation*, Sep. 10, 2025.
    [Online](https://docs.acescentral.com/system-components/input-transforms/).

[7] A. Foi, M. Trimeche, V. Katkovnik, and K. Egiazarian, “Practical
    Poissonian-Gaussian noise modeling and fitting for single-image raw-data,”
    *IEEE Trans. Image Process.*, vol. 17, no. 10, pp. 1737–1754, Oct. 2008,
    doi: 10.1109/TIP.2008.2001399.
    [Author institution](https://researchportal.tuni.fi/en/publications/practical-poissonian-gaussian-noise-modeling-and-fitting-for-sing/).

[8] P. Cousot and R. Cousot, “Abstract interpretation: A unified lattice
    model for static analysis of programs by construction or approximation
    of fixpoints,” in *Proc. 4th ACM Symp. Principles of Programming
    Languages*, 1977, pp. 238–252.
    [Author publication page](https://www.di.ens.fr/~cousot/COUSOTpapers/POPL77.shtml).

[9] P. M. Rondon, M. Kawaguchi, and R. Jhala, “Liquid types,” in *Proc. ACM
    SIGPLAN Conf. Programming Language Design and Implementation*, 2008,
    pp. 159–169, doi: 10.1145/1375581.1375602.
    [Author publication page](https://goto.ucsd.edu/~rjhala/papers/liquid_types.html).

[10] W3C, *WebGPU Shading Language*, sec. 15.7, “Floating point evaluation.”
    [Online](https://www.w3.org/TR/WGSL/#floating-point-evaluation).

[11] Tracel AI contributors, “CubeCL,” project documentation.
    [Repository](https://github.com/tracel-ai/cubecl).

[12] CIE, *A Method of Predicting Corresponding Colours under Different
    Chromatic and Illuminance Adaptations*, CIE 109-1994, 1994.
    [CIE description](https://www.cie.co.at/publications/method-predicting-corresponding-colours-under-different-chromatic-and-illuminance-0).

[13] C. Li et al., “Comprehensive color solutions: CAM16, CAT16, and
    CAM16-UCS,” *Color Res. Appl.*, vol. 42, no. 6, pp. 703–718, 2017,
    doi: 10.1002/col.22131.
    [Publisher](https://onlinelibrary.wiley.com/doi/full/10.1002/col.22131).

[14] K. He, J. Sun, and X. Tang, “Single image haze removal using dark
    channel prior,” in *Proc. IEEE Conf. Computer Vision and Pattern
    Recognition*, 2009, pp. 1956–1963, doi: 10.1109/CVPR.2009.5206515.
    [Author-hosted paper](https://people.csail.mit.edu/kaiming/publications/cvpr09.pdf).

### Darktable module references

Darktable contributors, *darktable User Manual*, development edition,
individual module references:

- [D1] [Color calibration](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/color-calibration/).
- [D2] [Sigmoid](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/sigmoid/).
- [D3] [Raw black/white point](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/raw-black-white-point/).
- [D4] [Demosaic](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/demosaic/).
- [D5] [White balance](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/white-balance/).
- [D6] [Highlight reconstruction](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/highlight-reconstruction/).
- [D7] [Denoise (profiled)](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/denoise-profiled/).
- [D8] [Input color profile](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/input-color-profile/).
- [D9] [Exposure](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/exposure/).
- [D10] [Color balance RGB](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/color-balance-rgb/).
- [D11] [Filmic RGB](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/filmic-rgb/).
- [D12] [Tone equalizer](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/tone-equalizer/).
- [D13] [Lens correction](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/lens-correction/).
- [D14] [LUT 3D](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/lut-3d/).
- [D15] [Negadoctor](https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/negadoctor/).

### vkdt module references

J. Hanika and vkdt contributors, *vkdt*, release 1.0.0, commit
`afcc804898715d05952066718f4e4c0eba21ed76`, module documentation. Links are
pinned to the inspected source revision.

- [V1] [Module reference](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/readme.md).
- [V2] [i-raw](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/i-raw/readme.md).
- [V3] [denoise](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/denoise/readme.md).
- [V4] [hotpx](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/hotpx/readme.md).
- [V5] [demosaic](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/demosaic/readme.md).
- [V6] [colour](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/colour/readme.md).
- [V7] [hilite](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/hilite/readme.md).
- [V8] [exposure](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/exposure/readme.md).
- [V9] [grade](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/grade/readme.md).
- [V10] [curves](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/curves/readme.md).
- [V11] [filmcurv](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/filmcurv/readme.md).
- [V12] [llap](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/llap/readme.md).
- [V13] [crop](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/crop/readme.md) and [lens](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/lens/readme.md).
- [V14] [mask](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/mask/readme.md) and [blend](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/blend/readme.md).
- [V15] [i-lut](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/i-lut/readme.md).
- [V16] [deconv](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/deconv/readme.md).
- [V17] [negative](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/negative/readme.md).
- [V18] [dehaze](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/dehaze/readme.md).
- [V19] [colenc](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/colenc/readme.md).
