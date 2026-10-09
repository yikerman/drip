# Independent port payloads and an extensible registry

Date: 2026-10-08. Status: **tentative formulation**, following R15–R23 in
[the user record](TYPE_SYSTEM_REDESIGN_USER_NOTES.md). This clarifies the
[Rust formulation](TYPE_SYSTEM_REDESIGN_RUST.md): `Value<P>` represents one
port value, not everything associated with an image or a node.

## Value, description, and node outputs

Each output port has its own payload family, host description, declared
placement, and eventual value. A node can produce several such outputs.
Their buffers, lifetimes, connections, and transfers are independent.

| Information | Where it belongs |
| --- | --- |
| Image samples | Image output's payload storage. |
| Image extent, CFA phase, coordinate identity, encoding, declared value convention | That image output's host description. |
| A color-transform matrix to apply | A separate matrix payload, with its own output port when produced by a node. |
| Matrix input/output coordinates | The matrix output's host description. |
| LUT samples | A separate LUT payload, not an image-descriptor member. |
| LUT grid shape, sampled domain, input/output interpretation | The LUT output's host description. |
| Gains, calibration levels, or noise coefficients produced for downstream use | Separate typed output values with descriptions of their declared applicability. |
| User-entered constants local to an operation | Typed node parameters; they need not become ports unless composition requires it. |

Size is not the distinction. Nine matrix coefficients are small but still
computational data. Metadata explains what a value means and which contracts
it can satisfy; it is not a bag of other results that downstream nodes may
implicitly retrieve. Coordinate definitions may contain numeric constants,
but that does not turn an independently selected transform into image metadata.

## Interpretation remains part of the design

Interpretation has not been replaced by storage types or discarded. The term
“descriptor” names the host record that carries interpretation together with
facts such as logical extent. It is not a replacement concept for meaning.

Some interpretation is expressed by the nominal payload family:
`CameraRgbF32` and `ColorRgbF32` differ despite using the same triple layout.
Other interpretation consists of runtime values: the particular camera
coordinates, RGB space, encoding, and sample scale. Make that distinction
explicit in descriptor definitions, for example:

```rust
struct ColorRgbDesc {
    extent: Extent2d,
    interpretation: RgbInterpretation,
}

struct RgbInterpretation {
    space: RgbSpaceId,
    encoding: Encoding,
    scale: SignalScale,
}
```

A matrix has its own interpretation: its declared source and target value
domains. The coefficients are its payload data. A matrix-application contract
compares the image's value domain with the matrix's source domain and derives
the output interpretation from its target domain. Geometry is handled
separately; the matrix does not need the image's extent in its value domain.

Node contracts are predicates and relations over these interpretations and
descriptors, plus separately identified conditions on actual values. Producers
establish the interpretations of their outputs. CPU/GPU transport preserves
them. The practical proposal omits broad physical-model/evidence tracking;
it still enforces the selected interpretation contracts.

## A concrete multi-output example

A `PrepareMatrixShaper` node can prepare a supported matrix-and-channel-curves
profile. Its named outputs are independent ports:

```rust
type RgbToXyzF32 = Matrix3x3F32<RgbInterpretation, XyzInterpretation>;

// Sketch: a generated collection of graph handles, not a packed data buffer.
struct MatrixShaperOutputs {
    decode_curves: Output<RgbCurveLutF32, Cpu>,
    matrix: Output<RgbToXyzF32, Cpu>,
}

struct ApplyRgbCurvesInputs {
    image: Input<ColorRgbF32, Gpu>,
    curves: Input<RgbCurveLutF32, Gpu>,
}

struct ApplyRgbToXyzInputs {
    image: Input<ColorRgbF32, Gpu>,
    matrix: Input<RgbToXyzF32, Gpu>,
}
```

The first consumer produces the declared decoded RGB. The second requires
that RGB domain to match the matrix's input and that the matrix's output is
the XYZ domain promised by that node. These are checks over separate input
descriptions. The evaluator uploads the curves and matrix separately when
their consumers need GPU representations.

This is a proposal for a specific profile-preparation operation, not an
automatic selector over arbitrary profile algorithms. A profile outside its
declared representation is rejected; it does not silently choose a different
transformation algorithm.

Darktable's inspected `colorin` implementation extracts a matrix and three
channel LUTs from an input profile [1]. That is evidence for these distinct
data products. Drip need not hide them inside the image-processing module or
inherit its fallback to another transformation implementation.

Another valid pattern is:

```text
FitCameraMatrix(camera_samples, reference_samples)
    -> matrix
    -> residuals

matrix -> ApplyCameraMatrix(image A, matrix)
       -> ApplyCameraMatrix(image B, matrix)

residuals -> inspect/export, or leave unconnected
```

This describes the outputs of one specified fitting algorithm. It does not
add residual analysis to every image node. A source/import node can likewise
expose image samples, available gains, and available calibration data through
different outputs rather than an image-attached bundle.

## Example payload catalogue

These are candidate independently connectable values, not a closed global
enum or a requirement to implement every row immediately. Introduce a family
when a supported node exposes or consumes that value. Each has concrete CPU
and GPU representations under `Payload`; layout names below denote a pair of
typed storage implementations, not one universal buffer.

### Rasters

| Payload family | Concrete logical storage | Host description and example use |
| --- | --- | --- |
| `BayerF32` | Scalar f32 plane | Extent, phase, camera-response coordinates, value convention. Raw normalization, Bayer hot-pixel filtering, Bayer demosaic. |
| `XTransF32` | Scalar f32 plane | Extent, X-Trans sampling information, response coordinates, value convention. A distinct X-Trans demosaic input. |
| `CameraRgbF32` | Dense f32 triples | Extent, camera-response coordinates and value convention. Demosaic output and camera-matrix input. |
| `ColorRgbF32` | Dense f32 triples | Extent, defined RGB coordinates, encoding, scale. Color grading, sigmoid, and RGB conversion. Rec.2020 and sRGB are descriptor values within this family. |
| `XyzF32` | Dense f32 triples | Extent, observer/coordinate convention and scale. Explicit RGB/XYZ conversion and adaptation operations. |
| `LabF32` | Dense f32 triples | Extent, reference white and Lab scaling convention. Lab operations such as those motivating darktable's color zones [2]. |
| `MaskF32` | Scalar f32 plane | Grid and declared weight convention. An explicit input to masked blending; it is not implicitly stored in the image's fourth channel. |
| `LuminanceF32` | Scalar f32 plane | Grid and luminance scale/reference convention. Luminance selection, tonal analysis, and local processing. |
| `TransmissionF32` | Scalar f32 plane | Grid and the declared transmission convention. An explicit dehaze input/output, not a distance map. |

These payloads may share low-level storage implementations without becoming
interchangeable. Numeric domain handling remains the selected algorithm's
contract; naming a payload does not establish that all its values are finite,
nonnegative, or in `[0,1]`.

### Transforms, calibration data, and other results

| Payload family | Concrete logical storage | Host description and example use |
| --- | --- | --- |
| `Matrix3x3F32<From, To>` | Nine f32 coefficients in a declared matrix layout | Concrete endpoint interpretation families, with runtime coordinate domains and value conventions. Camera conversion, RGB conversion, adaptation. Matrix dimensions alone are insufficient. |
| `RgbCurveLutF32` | Three explicitly sampled scalar curves | Sample grids/domains and declared RGB input/output interpretation. Matrix-shaper decoding or an explicitly defined channel-curve operation. |
| `ColorLut3dF32` | A 3D lattice of f32 triples | Grid dimensions, input sampling domain, input/output color interpretations. A specified LUT interpolation node; table shape does not identify its color meaning. |
| `SensorLevelsF32<N>` | Fixed arrays of black and white-reference values | Source sample convention and sensor channel/site mapping. Raw normalization; `N` is compile-time, not a runtime channel switch. |
| `ChannelGainsF32<N>` | Fixed array of f32 gains | Declared input/output response conventions and channel/site mapping. Explicit white-balance or gain application. |
| `AffineNoiseModelF32<N>` | Fixed arrays of coefficients for a stated variance model | Channel domain, sample scale, and the specified model convention. A profiled denoiser consumes it explicitly; this does not certify model accuracy after arbitrary edits. |
| `ColorSamplesF32` | A sequence of f32 triples | Coordinate domain and sample organization. Picked/reference patches; these need not be encoded as a narrow image. |
| `FitResidualsF32` | A sequence of residual triples | Comparison coordinates, scale, and sample pairing. A fitting result is distinguished from the measured/reference colors themselves. |
| `HistogramU32` | Array of u32 bin counts | Measured quantity, binning definition, and count convention. Exposure/statistical operations; not a rendered histogram picture. |
| `WarpMapF32` | A grid of f32 coordinate pairs | Map domain and source-coordinate convention. A fixed resampling operation consumes it explicitly. A displacement field would have a different declared contract. |

The exact Rust factoring can reuse structs or nominal generic wrappers.
That choice must preserve concrete instantiated storage and distinct checked
meanings. Do not introduce a universal `Tensor`/`Profile`/`AuxiliaryData` port
whose arbitrary contents require downstream guessing.

Calibration coefficients can be node parameters when fixed locally. When a
node computes, loads, or exports them for graph reuse, they are payloads and
edges express the dependency. Their numeric values are not required in host
metadata merely to make the connect-time checker convenient.

## Example node definitions

The following `#[node(contract = ...)]` syntax is a proposed extension, not
an implemented macro feature. The existing attribute macro supplies a style
reference, not semantic authority. `contract` is preferable to `describe`
because this function both rejects incompatible inputs and establishes output
descriptions. `subject_to` would suggest only a predicate.

`CpuRead<P>`/`GpuRead<P>` and `CpuWrite<P>`/`GpuWrite<P>` denote typed views of
concrete representations. The evaluator supplies immutable input views and
fresh private output views. Writes implement the pure value function without
mutating upstream values. The macro derives port families, placement, names,
and direction from the signature; it does not infer semantic constraints.
It checks that the named contract consumes the corresponding host descriptor
types and returns the declared output descriptor(s). Allocation, transport,
and dependency handling remain evaluator responsibilities.

All functions below omit computational bodies and some routine registration
fields. This example accepts omitted context/parameters when unused. Exact
wrapper names and generated adapters remain open. Descriptor-derived kernel
constants are prepared outside the pixel loop.

```rust
type CameraToXyzF32 = Matrix3x3F32<CameraRgbInterpretation, XyzInterpretation>;

/// Demosaic Bayer samples into camera RGB using the selected fixed algorithm.
#[node(id = "bayer_demosaic", contract = bayer_demosaic_contract)]
fn bayer_demosaic(
    ctx: &GpuContext,
    params: &BayerParams,
    mosaic: GpuRead<'_, BayerF32>,
    rgb: GpuWrite<'_, CameraRgbF32>,
) -> Result<(), EvalError> { /* bayer_demosaic_kernel */ }

/// Demosaic X-Trans samples into camera RGB using its own fixed algorithm.
#[node(id = "xtrans_demosaic", contract = xtrans_demosaic_contract)]
fn xtrans_demosaic(
    ctx: &GpuContext,
    params: &XTransParams,
    mosaic: GpuRead<'_, XTransF32>,
    rgb: GpuWrite<'_, CameraRgbF32>,
) -> Result<(), EvalError> { /* xtrans_demosaic_kernel */ }

/// Fit a 3×3 camera-to-XYZ matrix and report residuals for paired samples.
#[node(id = "fit_camera_matrix_least_squares", contract = camera_matrix_fit_contract)]
fn fit_camera_matrix_least_squares(
    params: &MatrixFitParams,
    measured: CpuRead<'_, ColorSamplesF32>,
    reference: CpuRead<'_, ColorSamplesF32>,
    matrix: CpuWrite<'_, CameraToXyzF32>,
    residuals: CpuWrite<'_, FitResidualsF32>,
) -> Result<(), EvalError> { /* least_squares_kernel */ }

/// Apply a matrix to camera RGB in its declared source convention; produce XYZ.
#[node(id = "apply_camera_to_xyz", contract = camera_to_xyz_contract)]
fn apply_camera_to_xyz(
    ctx: &GpuContext,
    image: GpuRead<'_, CameraRgbF32>,
    matrix: GpuRead<'_, CameraToXyzF32>,
    output: GpuWrite<'_, XyzF32>,
) -> Result<(), EvalError> { /* apply_matrix3_kernel */ }

/// Apply an RGB LUT by tetrahedral interpolation, clamping to its input domain.
#[node(id = "apply_tetrahedral_rgb_lut", contract = tetrahedral_rgb_lut_contract)]
fn apply_tetrahedral_rgb_lut(
    ctx: &GpuContext,
    image: GpuRead<'_, ColorRgbF32>,
    lut: GpuRead<'_, ColorLut3dF32>,
    output: GpuWrite<'_, ColorRgbF32>,
) -> Result<(), EvalError> { /* tetrahedral_rgb_lut_kernel */ }

/// Compute a + weight * (b - a) in matching RGB coordinates; allow extrapolation.
#[node(id = "mix_rgb", contract = rgb_mix_contract)]
fn mix_rgb(
    ctx: &GpuContext,
    a: GpuRead<'_, ColorRgbF32>,
    b: GpuRead<'_, ColorRgbF32>,
    weight: GpuRead<'_, MaskF32>,
    output: GpuWrite<'_, ColorRgbF32>,
) -> Result<(), EvalError> { /* rgb_mix_kernel */ }
```

Each definition selects one specified algorithm. `BayerParams` and
`XTransParams` contain values for their respective implementations, not an
algorithm selector. If multiple demosaic algorithms are offered, each gets
its own definition with whichever contract it actually needs.

| Definition | Checks and output interpretation |
| --- | --- |
| `BayerDemosaic` | Requires a Bayer payload with known current phase and any additional sample-convention/extent requirements of that specific kernel. Output is camera RGB in the algorithm's declared response convention and extent; it does not acquire standard RGB primaries. |
| `XTransDemosaic` | Requires X-Trans sampling information and that kernel's declared sample convention/extent. Produces camera RGB. It never substitutes the Bayer implementation. |
| `FitCameraMatrixLeastSquares` | This example fits an unregularized 3×3 matrix. Requires measured samples in declared camera-response coordinates and reference samples in declared XYZ coordinates, with matching count, at least three sample pairs, and declared correspondence. Output matrix maps those measured coordinates/conventions to those XYZ coordinates/conventions. Residuals describe errors in the comparison coordinates. Rank deficiency and other conditions on measured values are handled during evaluation, not inferred from descriptors. |
| `ApplyCameraToXyz` | Requires the image's response domain to match the matrix's source domain. The matrix payload's endpoint types already require camera RGB to XYZ. Output keeps the image extent and adopts the declared target interpretation. A camera-to-RGB matrix cannot satisfy this input family. |
| `ApplyTetrahedralRgbLut` | Requires image interpretation to match the LUT's input and the LUT output to declare RGB. Checks the lattice dimensions required by this interpolation algorithm. Output keeps image geometry and adopts the LUT's declared output interpretation. This example defines clamping to the LUT input domain; another algorithm's boundary behavior must be explicit. There is no interpolation-method selector. |
| `MixRgb` | Requires matching RGB value domains and matching extents for this elementwise operation; weight extent and its zero/one convention must match the operation. Computes `a + w * (b - a)` and retains the RGB domain. This definition allows extrapolation for weights outside `[0,1]`; it does not assert physical registration or insert color conversion/resampling. |

## An explicit connect-time check

The matrix family can carry concrete endpoint interpretation types without
changing the nine-coefficient CPU/GPU layouts. This refines the earlier bare
`Matrix3x3F32` notation: each instantiation has fixed endpoint families, while
particular camera coordinates and XYZ conventions remain runtime values.
Adding a new endpoint family requires no global enum or matrix-kernel branch.

For this example, the host types include:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
struct CameraRgbInterpretation {
    coordinates: CameraCoordinatesId,
    encoding: Encoding,
    scale: SignalScale,
}

struct Matrix3x3Desc<From, To> {
    source: From,
    target: To,
}
```

`CameraCoordinatesId` identifies the declared response coordinates and channel
convention, not just the manufacturer/model name. Equality uses canonical
semantic identifiers and declared conventions, not approximate comparison of
profile coefficients or equality of provenance. `Encoding` and `SignalScale`
are small checked values/identities; their full vocabularies remain open.

The contract function contains the actual relational check:

```rust
fn camera_to_xyz_contract(
    image: &CameraRgbDesc,
    matrix: &Matrix3x3Desc<CameraRgbInterpretation, XyzInterpretation>,
) -> Result<XyzDesc, ContractError> {
    if image.interpretation != matrix.source {
        return Err(ContractError::DomainMismatch {
            port: "image",
            required_by: "matrix.source",
            expected: matrix.source.clone(),
            actual: image.interpretation.clone(),
        });
    }

    Ok(XyzDesc {
        extent: image.extent,
        interpretation: matrix.target.clone(),
    })
}
```

This is ordinary CPU Rust code. The illustrative error carries concrete
camera interpretations; a generated adapter can format it for the graph UI.
The comparison checks the fields above, not entire descriptors with unrelated
geometry. Payload admission already establishes the fixed matrix shape and
well-formed descriptors. No determinant check is needed for multiplication:
a singular matrix is still a valid forward linear map.

| Image interpretation | Matrix source interpretation | Result |
| --- | --- | --- |
| Camera A response coordinates, identity encoding, normalized scale | Same | Accept; output extent comes from the image and XYZ interpretation from the matrix target. |
| Camera B response coordinates | Camera A response coordinates | Reject with the two differing domains. |
| Camera A, normalized scale | Camera A, ADC-count scale | Reject even though both images have f32 triple storage. |
| Camera A, nonlinear encoding | Camera A, identity encoding | Reject; no decoding is inserted. |

If the matrix is already connected, adding the image edge runs this check
before committing. The reverse connection order gives the same result. While
one required input is absent, only available per-port checks can run; the
node is incomplete and has no promised output descriptor in this example.
Adding the missing input must pass the relation. Replacing either source or
changing descriptor-relevant parameters rechecks affected downstream edges
transactionally. Unknown required interpretation is not a wildcard.

The contract does not inspect matrix coefficients or pixels. During evaluation
the adapter supplies concrete GPU input views and a private output allocation
to the multiplication kernel. The matrix remains a separate computational
value even though its endpoint interpretations are CPU-accessible.

These checks trust correctly constructed producer descriptions. Neither the
attribute nor descriptor equality proves a fitted matrix is an accurate
physical camera model. Creative edits need not invalidate a coordinate domain
merely because they change pixel values. That limitation is intentional under
the practical coverage requirement.

Connecting the CPU matrix output of the fitting node to the GPU matrix input
of the application node records an upload requirement. The evaluator performs
that upload. The same matrix output can feed several compatible image branches;
the residuals output can be consumed separately or left unconnected.

The same principle applies to the earlier `PrepareMatrixShaper`: its curves
and matrix outputs remain independently usable even though one algorithm
prepares both. Multiple outputs are not a reason to create a universal node
result interpretation shared by all ports.

## Extending the system

A new payload implementation supplies concrete CPU/GPU storage mappings,
a concrete descriptor type, a registered schema identity, and its supported
representation adapters. Its nodes supply their own descriptor checks and
propagation rules. The evaluator works through registered adapters; adding a
payload does not require a new branch in every existing node or a new arm in
a global payload enum. Registration may use Rust type erasure internally;
the computational entry points still receive concrete types.

For example, a future deconvolution node with an external PSF can add
`PsfKernelF32`, whose payload contains kernel samples and whose descriptor
defines sampling and origin. That is an extension, not a reason to store a
PSF in every image description. The inspected vkdt deconvolution instead
takes an image and a Gaussian-width parameter [3]; its internal blur work
does not by itself require a public PSF port.

The same applies to spectra, neural-network weights, or geometric data when
those operations are actually introduced. Their absence from this example
catalogue does not limit the evaluator to photography rasters.

## Reuse, unused outputs, and absence

Each output may have zero, one, or several consumers. Within an evaluation,
consumers share the immutable value; the evaluator retains CPU/GPU
materializations only as required by consumers and their placement. This
does not introduce a persistent DAG cache.

An unconnected auxiliary output does not make the node invalid. It does not
need a transfer or long-lived storage for nonexistent consumers. However,
multiple outputs from one algorithm do not automatically become independently
computable. The first implementation may compute the fixed result tuple and
release unused results. Skipping expensive output production needs an
explicit computation decomposition, not guessed behavior or a new collection
of `need_matrix`/`need_lut` branches inside processing kernels.

An unused output and an unavailable value are different. If an imported asset
does not supply a calibration result, do not invent that result or substitute
an identity transform. Expose its known unavailability at binding/connect
time; genuinely unknown availability requires an explicit evaluation-time
condition. Optional reuse does not require nullable image payloads or implicit
fallback selection by consumers.

## Upstream examples are evidence, not the architecture

vkdt already supports multiple connectors; that is not the missing feature.
The inspected `dt_image_params_t` nevertheless puts a camera conversion
matrix, white-balance coefficients, and noise coefficients alongside image
interpretations. It is stored once per module, and its comment acknowledges
the need for per-connector information [4]. This makes some computational
dependencies implicit and is a poor fit for independently reusable, checked
outputs. Drip should not copy that mixed structure.

Likewise, vkdt's `i-lut` accepts general texture data [5]. Loading a buffer
through something called “LUT” does not make it a color-transform lattice.
`ColorLut3dF32` is one explicit payload family, not a label for every table.

The dehaze example demonstrates why we must inspect the mathematics: its
`depth` shader computes a transmission estimate, and its consumer uses that
value as `t` in `(I - A) / t + A` [6]. A Drip transmission output should be
separate from the corrected image and carry its own interpretation. It does
not belong in the image descriptor and does not become distance in meters.

This session changes only redesign documents. No kernels, payload registry,
or graph implementation were added; no existing project logs were updated.

## References

[1] darktable developers, “Input color profile implementation,” revision
`733bd69f32cac7ff5e41025115942772add1f088`, accessed Oct. 8, 2026.
[Online]. Available: [matrix and channel-LUT extraction](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/colorin.c#L1706).

[2] darktable developers, “Color zones implementation,” same revision,
accessed Oct. 8, 2026. [Online]. Available:
[declared Lab processing domain](https://github.com/darktable-org/darktable/blob/733bd69f32cac7ff5e41025115942772add1f088/src/iop/colorzones.c#L167).

[3] J. Hanika and contributors, “Deconvolution module,” vkdt 1.0.0, revision
`afcc804898715d05952066718f4e4c0eba21ed76`, accessed Oct. 8, 2026.
[Online]. Available: [module description](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/deconv/readme.md).

[4] J. Hanika and contributors, “Module data structures,” same vkdt revision,
accessed Oct. 8, 2026. [Online]. Available:
[image parameters and module ownership](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/module.h#L52).

[5] J. Hanika and contributors, “Lookup table input,” same vkdt revision,
accessed Oct. 8, 2026. [Online]. Available:
[module description](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/i-lut/readme.md).

[6] J. Hanika and contributors, “Dehaze shaders,” same vkdt revision,
accessed Oct. 8, 2026. [Online]. Available:
[transmission estimator](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/dehaze/depth.comp),
[dehazing operation](https://github.com/hanatos/vkdt/blob/afcc804898715d05952066718f4e4c0eba21ed76/src/pipe/modules/dehaze/dehaze.comp).
