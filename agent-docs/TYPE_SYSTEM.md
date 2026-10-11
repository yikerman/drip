# Type system: principles and mathematics

The user's principles for nodes, ports and contracts, and the mathematics that
bounds what contracts can establish. Design proposals and implementation
records from the 2026-10-08 redesign were removed; the code is the current
implementation, and open questions are in [DESIGN](DESIGN.md). General code
principles are in `AGENTS.md`.

## Principles

User statements, paraphrased.

### Nodes and graph

- A node is a pure function `(global context, parameters, input ports) ->
  output ports`, an ordinary operation plus the machinery a DAG needs. "Minimum
  work" in a node means repeated graph and execution logic belongs in DAG/eval,
  not that algorithms are split into their finest units.
- Each algorithm does one thing. Distinct algorithms are distinct nodes (Bayer
  and X-Trans demosaic are two nodes). There is no guessing in a node: it
  executes its declared contract; incompatible input never triggers algorithm
  substitution. Minimize branching.
- A node may produce several outputs. Independently usable values, such as a
  color matrix or LUT, are separate typed ports, not packed into one buffer or
  into metadata. Outputs may be optionally reused; payloads are extensible.
- The global context passed to every node governs processing such as preview
  scale. Demosaic performs the requested reduction internally; no hidden nodes.
- View and action nodes are ordinary node declarations with an empty
  computation, declaring parameters and inputs like any other node.
- GUI nodes are generated from node definitions, with overridden or extra
  behavior. Preserve universal node controls/views and per-node pop-outs.
- The graph is acyclic and evaluated on demand: `eval(dag, global, target)`,
  requested by the GUI, export or other consumers.
- No evaluation cache by default; large buffers can exhaust host/device memory
  at a working scale of about 4 GB. Stay compatible with future caching that
  preserves purity, such as a RAW reader cache.

### Ports and contracts

- Each port carries a concrete buffer or data structure with CPU and GPU
  representations, which need not share a memory layout. Each supported logical
  type has a bijection between them. Kernel inputs and outputs have concrete
  Rust types known at compile time, not abstract algebraic structures.
- Port values still need interpretation, assumption and capability checks.
  Interpretation remains part of the design.
- Reject only a practical subset of logically wrong graphs; catch ordinary
  errors and leave algorithm nuances out. Creative processing stays allowed.
  vkdt's buffer checks are too loose; check substantially more while staying
  clean. Do not assume darktable or vkdt solved these problems correctly.
- Failing connections are unconnectable at connect time, distinct from compile
  time and evaluation time. Where strict connect-time checking is impossible,
  move specific checks to evaluation.
- DAG checks run on the CPU. Metadata, interpretations and constraints are
  logically grouped with a buffer but physically separate from image storage.
- Placement is known at connect time; the evaluator owns CPU/GPU movement.
- Same-placement edges and fan-out share immutable storage. Evaluation
  materializes a missing representation once per output within a run.
- Keep concrete Rust types: uniform movement does not mean homogeneous buffers
  or opaque bytes. Replace incompatible fields locally (UTF-8 text and ICC data
  can be byte vectors).

### Inputs and approximation

- A mosaic is a two-dimensional scalar array, informally related to collected
  photons and sensor capacity, with losses from QE, shot noise and read noise.
  "Fraction of full-well capacity" may claim excessive physical accuracy.
- Fully accurate physical information is generally unobtainable. Missing dark
  or flat fields, sensor profiles and other calibration must not prevent
  ordinary processing; later nodes may work on a sufficient approximation,
  including as-shot white balance and highlight recovery.

### Work order

- Build a correct, clean node/port/DAG/eval model first, then adapt the UI,
  then optimize. Do not optimize prematurely or overcomplicate.
- Tracing must not change scheduling or add synchronization.
- Long, nested functions trigger a responsibility review, not cosmetic helper
  extraction.

## Mathematics

### What a check can establish

"Logically wrong" covers several questions:

1. Is the operation defined on these representations and numerical values?
2. Do the supplied coordinates, units and reference data meet its contract?
3. Is its model a good approximation to this photograph?
4. Is the result desirable?

The first two admit checks. The third often involves unobserved facts: an
image cannot prove that its illuminant was uniform, that a surface was
neutral, or that a profile fits the camera's spectra. The fourth belongs to the
user. No complete classifier exists for arbitrary programs and semantic
predicates; restricting the contract language makes particular questions
decidable without making physical truth observable [8]. Checks therefore
establish consistency with explicit contracts. Allocation, I/O and device
failures are execution failures, not proofs of logical invalidity.

Three kinds of statement differ:

- **Definition:** these numbers are linear Rec.2020 coordinates with a stated
  scale. This assigns meaning; it does not prove scene accuracy.
- **Checkable fact:** shapes match, identifiers agree, denominators are nonzero.
- **Modeling assumption:** a response is adequately linear in a region, an
  estimated illuminant is appropriate, a noise model is useful. An assumption
  has a source and scope; it is not promoted to a measured fact.

Global tags such as `Linear`, `Calibrated` or `ValidColor` hide their
arguments. Useful claims name them: "this profile consumes these sensor
coordinates", "this mask is registered to this grid".

### Observation and normalization

A simplified observation model [4], [7]:

```text
N_p ~ Poisson(mu_p)
mu_p = integral eta_p(lambda) Phi_p(lambda, t) d(lambda) dt
d_p  = quantize_and_clip(b_p + g_p (N_p + noise_p))
```

`Phi` is incident photon rate per wavelength at a photosite, `eta` its
spectral response, `g` the electron-to-code gain and `b` a modeled offset. Dark
signal and other disturbances need further terms. Normalization makes the much
narrower statement

```text
r_p = (linearize(d_p) - B_p) / (W_p - B_p),   W_p > B_p,
```

with `B` and `W` in the same linearized code coordinates. This defines zero and
a clipping reference without claiming electron counts, radiance or a full-well
fraction; EMVA distinguishes digital saturation capacity from full-well
capacity [4]. Samples need not lie in `[0, 1]`: noise, estimated levels and
later gains produce values outside it. QE and noise do not prevent a linear
model of the mean response in an unsaturated region; missing calibration
reduces confidence without making the representation undefined.

The minimum useful raw interpretation is samples of named sensor responses on
a stated sampling lattice, with a stated encoding, offset/scale convention and
clipping information where an algorithm needs it.

### Sensor responses are not tristimulus coordinates

Discretize a spectrum as `s`. Camera responses are `q = Q s` and an observer's
tristimulus values `x = C s`. An exact universal matrix conversion requires

```text
C = M Q.
```

Three components on both sides do not imply this. If `Q z = 0` but `C z != 0`
for some perturbation `z`, the camera cannot distinguish spectra the observer
distinguishes, and no annotation recovers that information (Luther condition
[5]). A camera profile is a conversion model, usually approximate over an
intended domain. Its output has well-defined coordinates while remaining an
estimate of scene colorimetry; a type system need not certify a perfect camera
to permit it [6].

Consequently, three sensor channels support gains, mixing and scalar curves
without colorimetry. What needs additional information is a justified use of
Rec.2020 luminance weights, perceptual hue, or a renderer whose primaries have
colorimetric meaning. An explicit reinterpretation can assign Rec.2020 meaning
to camera numbers, but cannot claim to have calibrated the camera.

### Encoding, editing and capture relations

Stored values are `v = E(c)` for coordinates `c` and encoding `E`. A nonlinear
creative edit `c_new = G(c)` produces new coordinates with the same identity
encoding; the storage interpretation does not become `E = G`. What may cease to
hold is a capture relation such as `c ≈ M q`, or proportionality to estimated
scene light. Identity encoding does not assert that every preceding operation
was linear.

Scene-referred does not mean linearly encoded. CIE defines scene- and
output-referred states by estimated scene coordinates versus rendered output
for specified viewing conditions [1], [2]; ACEScct is a logarithmic
scene-referred grading encoding [3]. Sigmoid can produce linearly encoded,
display-referred output [D2]. The RGB reference white, the scene's estimated
illuminant and the observer's adopted white are distinct, even when all are D65.

A full-adaptation von Kries-style transform in XYZ, with whites at a common
luminance normalization:

```text
A = H^-1 diag((H W_destination) / (H W_source)) H,   x_out = A x_in.
```

It needs defined coordinate/white conventions, invertible `H` and valid
denominators. CATs predict corresponding colors under their models; they are
not spectral relighting [10], [11]. For an invertible camera model `x = M q`,
the camera-coordinate equivalent is `M^-1 A M`, so adaptation before camera
conversion is meaningful. For a nonlinear rendering `F`, generally
`A F(x) != F(A x)`; transporting the same correction through `F` needs
`F A F^-1`, which clipping can make unavailable. Adapting the rendered image is
a different, definable operation. Color calibration combines adaptation,
mixing and calibration, with documented valid exceptions to the usual order
[D1]. Applying unchanged capture-calibration parameters after a nonlinear
rendering is a model/input mismatch, not a universal ordering ban.

### Auxiliary values have coordinates

For a scalar noise model `Var(x | mu) = a mu + b`, scaling `y = k x` gives

```text
Var(y | mu_y) = (k a) mu_y + k^2 b,
```

so copied coefficients are wrong. Channel mixing transforms covariance as
`Sigma_y = M Sigma_x M^T`, correlating independent channels; resampling
introduces spatial correlation [7]. Likewise, clipping thresholds follow gains,
a crop updates CFA phase, and a mask follows its image's geometry. A value
alone does not reveal whether it was clipped earlier. Preserve the evidence a
consumer needs, or withdraw the corresponding guarantee.

### Representations and purity

A CPU array and a padded GPU array need not be in bijection as byte strings;
they can represent the same logical sample sequence. The required bijection is
lossless transport between supported logical values. `f32` to `f16` is a
precision-changing computation, not transport. Exact transport also does not
imply bit-identical CPU/GPU kernel results; WGSL permits floating-point
behavior that must be considered explicitly [9].

A matrix's layout and scalar type can be known at compile time while its
coefficients, camera identity and dimensions are known only at evaluation.
Concrete schemas suffice for that separation.

A path is not immutable file contents: reading it twice can return different
bytes. Source identity must refer to a frozen asset version, or loading must
precede pure decoding. Profiles, seeds and semantic context are fixed per
evaluation. Without a persistent cache, live intermediates, branch retention,
scratch and queued GPU work still occupy memory: a 100-megapixel four-channel
`f32` image is about 1.49 GiB, so three exceed 4 GiB.

## References

[1] CIE, "Scene-referred image state," *International Lighting Vocabulary*,
CIE S 017:2020, term 17-32-054, 2020. [Online]. Available:
<https://cie.co.at/eilvterm/17-32-054>

[2] CIE, "Output-referred image state," *International Lighting Vocabulary*,
CIE S 017:2020, term 17-32-052, 2020. [Online]. Available:
<https://cie.co.at/eilvterm/17-32-052>

[3] Academy of Motion Picture Arts and Sciences, "ACES system," *ACES
Documentation*, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/background/overview/>

[4] European Machine Vision Association, *Standard for Characterization of
Image Sensors and Cameras*, EMVA 1288, Release 4.0 Linear, Jun. 16, 2021,
secs. 2–3. [Online]. Available:
<https://www.emva.org/wp-content/uploads/EMVA1288Linear_4.0Release.pdf>

[5] G. D. Finlayson and Y. Zhu, "Designing color filters that make cameras
more colorimetric," *IEEE Trans. Image Process.*, vol. 30, pp. 853–867, 2021,
doi: 10.1109/TIP.2020.3038523.

[6] Academy of Motion Picture Arts and Sciences, "Input transforms," *ACES
Documentation*, Sep. 10, 2025. [Online]. Available:
<https://docs.acescentral.com/system-components/input-transforms/>

[7] A. Foi, M. Trimeche, V. Katkovnik and K. Egiazarian, "Practical
Poissonian-Gaussian noise modeling and fitting for single-image raw-data,"
*IEEE Trans. Image Process.*, vol. 17, no. 10, pp. 1737–1754, Oct. 2008,
doi: 10.1109/TIP.2008.2001399.

[8] P. Cousot and R. Cousot, "Abstract interpretation: A unified lattice model
for static analysis of programs by construction or approximation of
fixpoints," in *Proc. 4th ACM Symp. Principles of Programming Languages*, 1977,
pp. 238–252.

[9] W3C, *WebGPU Shading Language*, sec. 15.7, "Floating point evaluation."
[Online]. Available: <https://www.w3.org/TR/WGSL/#floating-point-evaluation>

[10] CIE, *A Method of Predicting Corresponding Colours under Different
Chromatic and Illuminance Adaptations*, CIE 109-1994, 1994.

[11] C. Li et al., "Comprehensive color solutions: CAM16, CAT16, and
CAM16-UCS," *Color Res. Appl.*, vol. 42, no. 6, pp. 703–718, 2017,
doi: 10.1002/col.22131.

[D1] darktable contributors, "Color calibration," *darktable User Manual*,
development edition. [Online]. Available:
<https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/color-calibration/>

[D2] darktable contributors, "Sigmoid," *darktable User Manual*, development
edition. [Online]. Available:
<https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/sigmoid/>
