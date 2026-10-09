# Third-party credits

Drip is AGPL-3.0-or-later. We thank the authors and contributors of the projects
below.

Rust dependency versions are recorded in `Cargo.lock`; native source pins are
listed below. This lists direct dependencies and major native
components, rather than duplicating the full transitive dependency tree.

## Processing sources

Adapted code retains its upstream copyright and license notices. References
beside each algorithm identify the source and explain changes made for Drip.

| Source and credit | Use | License |
|-------------------|-----|---------|
| [darktable developers](https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3), including sigmoid and custom-primaries contributors | Sigmoid curve, hue/energy correction and primaries handling, adapted to Rust/CubeCL | GPL-3.0-or-later; [license text](https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/LICENSE) |
| [Luis Sanz Rodríguez](https://github.com/LuisSR/RCD-Demosaicing), Ingo Weyrich, Hanno Schwalm and darktable contributors | RCD demosaicing, adapted from the pinned darktable revision above; bilinear border in Drip | Original RCD: GPL-3.0; darktable integration: GPL-3.0-or-later |
| garagecoder and Iain (G’MIC), Hanno Schwalm and [darktable contributors](https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/hlreconstruct) | Inpaint-opposed highlight reconstruction; Bayer adaptation with complete edge neighborhoods | GPL-3.0-or-later |
| [Dave Coffin, dcraw](https://www.dechifro.org/dcraw/) | Reference for camera matrix normalization and RAW black-level conventions, through LibRaw | dcraw's source contains multiple licensing options; credited here as an algorithm reference, not a vendored dependency |

Further algorithm ports must add their authors, exact source revision and license
here when introduced.

## Runtime dependencies

| Project and credit | Use | License |
|--------------------|-----|---------|
| [LibRaw LLC and contributors](https://www.libraw.org/) | Bundled RAW decoding through Drip's C shim | LGPL-2.1 OR CDDL-1.0; [upstream notices](https://github.com/LibRaw/LibRaw/blob/b93f6e45c194f5df9b02a43b1af9a54b4f41f33f/COPYRIGHT), [LGPL](https://github.com/LibRaw/LibRaw/blob/b93f6e45c194f5df9b02a43b1af9a54b4f41f33f/LICENSE.LGPL), [CDDL](https://github.com/LibRaw/LibRaw/blob/b93f6e45c194f5df9b02a43b1af9a54b4f41f33f/LICENSE.CDDL) |
| [D. R. Commander and libjpeg-turbo contributors](https://libjpeg-turbo.org/), Independent JPEG Group | Static JPEG codec for LibRaw; this software is based in part on the work of the Independent JPEG Group | IJG, BSD-3-Clause and Zlib; [license overview](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/c85e6b905bf237038faa936dab160ebfc5da0344/LICENSE.md), [IJG terms](https://github.com/libjpeg-turbo/libjpeg-turbo/blob/c85e6b905bf237038faa936dab160ebfc5da0344/README.ijg) |
| [Jean-loup Gailly, Mark Adler and zlib contributors](https://zlib.net/) | Static deflate codec for LibRaw | Zlib; [license](https://github.com/madler/zlib/blob/da607da739fa6047df13e66a2af6b8bec7c2a498/LICENSE) |
| [Marti Maria and Little CMS contributors](https://github.com/mm2/Little-CMS) | ICC export, preview softproofing and gamut classification | MIT |
| [Kornel Lesiński and rust-lcms2 contributors](https://github.com/kornelski/rust-lcms2) (`lcms2`, `lcms2-sys`) | Rust Little CMS bindings | MIT |
| [David Tolnay and linkme contributors](https://github.com/dtolnay/linkme/tree/0.3.37) (`linkme` 0.3.37) | Linked node discovery and local custom GUI bindings | MIT OR Apache-2.0 ([MIT](https://github.com/dtolnay/linkme/blob/0.3.37/LICENSE-MIT), [Apache-2.0](https://github.com/dtolnay/linkme/blob/0.3.37/LICENSE-APACHE)) |
| [Rayon contributors](https://github.com/rayon-rs/rayon) | Processing and GUI scope/proofing parallelism | MIT OR Apache-2.0 |
| [Tokio/bytes contributors](https://github.com/tokio-rs/bytes) (`bytes`) | Shared host allocation ownership passed to CubeCL's existing transfer controller | MIT; [license](https://github.com/tokio-rs/bytes/blob/master/LICENSE) |
| [image-rs contributors](https://github.com/image-rs/image-tiff) (`tiff`) | TIFF encoding and GUI proof/export parity tests | MIT |
| [Emil Ernerfeldt and egui contributors](https://github.com/emilk/egui) (`egui`, `egui-winit`, `egui-wgpu`) | GUI and renderer integration | MIT OR Apache-2.0 |
| [gfx-rs contributors](https://github.com/gfx-rs/wgpu) (`wgpu`, patched below) | GPU rendering | MIT OR Apache-2.0 |
| [rust-windowing contributors](https://github.com/rust-windowing/winit) (`winit`) | Windows and input events | Apache-2.0 |
| [Smithay contributors](https://github.com/Smithay/wayland-rs) (`wayland-client`, `wayland-protocols`) | Wayland pop-out parenting and surface color descriptions | MIT |
| [PolyMeilex and rfd contributors](https://github.com/PolyMeilex/rfd) | Native file dialogs | MIT |
| [Serde contributors](https://github.com/serde-rs/serde) (`serde`, `serde_json`) | Project serialization | MIT OR Apache-2.0 |
| [David Tolnay and thiserror contributors](https://github.com/dtolnay/thiserror) | Error definitions | MIT OR Apache-2.0 |
| [Rust log contributors](https://github.com/rust-lang/log), [env_logger contributors](https://github.com/rust-cli/env_logger) | Logging | MIT OR Apache-2.0 |
| [half-rs contributors](https://github.com/VoidStarKat/half-rs) (`half`) | Half-float texture preparation | MIT OR Apache-2.0 |
| [Joshua Barretto and pollster contributors](https://github.com/zesterer/pollster) | Blocking GPU initialization | MIT OR Apache-2.0 |

## Bundled native sources

Native sources are unmodified Git submodules at release tags. Gitlinks pin the
exact commits; updating a tag upstream does not change a Drip checkout.

| Directory | Release tag | Commit |
|-----------|-------------|--------|
| `vendor/libraw` | `0.22.2` | `b93f6e45c194f5df9b02a43b1af9a54b4f41f33f` |
| `vendor/libjpeg-turbo` | `3.2.0` | `c85e6b905bf237038faa936dab160ebfc5da0344` |
| `vendor/zlib` | `v1.3.2` | `da607da739fa6047df13e66a2af6b8bec7c2a498` |

`drip-raw` uses LibRaw's upstream translation-unit list [18], reentrant settings
and JPEG/zlib support, with no OpenMP, internal LittleCMS, RawSpeed or DNG SDK.
The codecs use their upstream CMake builds with static linkage and without tools
or tests; JPEG SIMD is required and its embedded build identifier is `drip`.
LittleCMS 2.19 is bundled by the locked `lcms2-sys` 4.0.7 crate and built through
its `static` feature; it has no separate submodule.

The submodules retain their own copyright/license files. Source distributions
must include their contents, not just Git links.

## Patched dependencies

wgpu is pinned to [yikerman/wgpu `e74560774b6029e893dab73961c2dbc57797cd78`](https://github.com/yikerman/wgpu/commit/e74560774b6029e893dab73961c2dbc57797cd78),
a `PassThrough` addition to v30.0.1 (`40f4a34e`) for egui-wgpu 0.36 compatibility
[13]. The fork retains wgpu's MIT OR Apache-2.0 licensing
([MIT](https://github.com/yikerman/wgpu/blob/e74560774b6029e893dab73961c2dbc57797cd78/LICENSE.MIT),
[Apache-2.0](https://github.com/yikerman/wgpu/blob/e74560774b6029e893dab73961c2dbc57797cd78/LICENSE.APACHE)).

Remove the workspace `[patch.crates-io]` override when upgrading to a compatible
release with passthrough support.

## Build and test dependencies

| Project and credit | Use | License |
|--------------------|-----|---------|
| [David Tolnay and Rust macro contributors](https://github.com/dtolnay/syn) (`syn`, `quote`, `proc-macro2`; versions in Cargo.lock) | Local `drip-macros` node/parameter declaration parsing, typed adapters and GUI binding discovery | MIT OR Apache-2.0 ([syn licenses](https://github.com/dtolnay/syn#license), [quote licenses](https://github.com/dtolnay/quote#license), [proc-macro2 licenses](https://github.com/dtolnay/proc-macro2#license)) |
| [clap contributors](https://github.com/clap-rs/clap) | Packaging task argument parsing | MIT OR Apache-2.0; [licenses](https://github.com/clap-rs/clap#license) |
| [cargo_metadata contributors](https://github.com/oli-obk/cargo_metadata) | Cargo executable artifact discovery for packaging | MIT; [license](https://github.com/oli-obk/cargo_metadata/blob/main/LICENSE) |
| [cc-rs contributors](https://github.com/rust-lang/cc-rs) (`cc`) | Compile LibRaw and the C shim | MIT OR Apache-2.0 |
| [cmake-rs contributors](https://github.com/rust-lang/cmake-rs) (`cmake`) | Cargo integration for the upstream JPEG/zlib CMake builds | MIT OR Apache-2.0 |
| [pkg-config-rs contributors](https://github.com/rust-lang/pkg-config-rs) | Find native libraries | MIT OR Apache-2.0 |
| [egui contributors](https://github.com/emilk/egui) (`egui_kittest`) | Headless GUI tests | MIT OR Apache-2.0 |

Test images in `fixtures/raw/pixls/` are from [raw.pixls.us](https://raw.pixls.us/),
licensed [CC0](https://creativecommons.org/publicdomain/zero/1.0/).
Thank you to the contributors—you guys are awesome!

Original Sony ILCE-7RM3 photo (`fixtures/raw/sony-ilce-7rm3.arw`):
Yi Cao <yi@ycao.net>.

## Papers and technical references

References used in processing and colorimetry; individual source modules cite
these works where the formulas are implemented.

[1] K.-I. Naka and W. A. H. Rushton, “S-potentials from colour units in the
retina of fish (Cyprinidae),” *J. Physiol.*, vol. 185, no. 3, pp. 536–555, 1966.
Historical basis for Drip's original sigmoid curve.

[2] ITU-R, “Parameter values for ultra-high definition television systems for
production and international programme exchange,” Rec. ITU-R BT.2020-2,
Oct. 2015. Rec.2020 primaries and white point.

[3] SMPTE, “Derivation of basic television color equations,” SMPTE RP 177-1993,
1993. RGB/XYZ matrix construction.

[4] ITU-R, “Parameter values for the HDTV standards for production and
international programme exchange,” Rec. ITU-R BT.709-6, Jun. 2015.
BT.709/sRGB primaries.

[5] SMPTE, “D-Cinema quality – Reference projector and environment,”
SMPTE RP 431-2:2011, 2011. P3 primaries; Drip uses D65 for Display P3.

[6] CIE, “CIE 1976 uniform-chromaticity-scale diagram,” *International Lighting
Vocabulary*, term 17-23-073. [Online]. Available:
https://cie.co.at/eilvterm/17-23-073. Vectorscope u′v′ equations; implemented
directly in Rust, using the input RGB space’s transform to D65 XYZ.

[7] darktable developers, “Scopes,” *darktable user manual*, development edition.
[Online]. Available:
https://docs.darktable.org/usermanual/development/en/module-reference/utility-modules/shared/scopes/.
Scope behavior reference, accessed Oct. 5, 2026; no source code copied.
Drip uses exposure-independent u′v′ chromaticity and an EV waveform.

[8] Y. Zhu and G. D. Finlayson, “A Mathematical Investigation into the Design
of Prefilters That Make Cameras More Colorimetric,” *Sensors*, vol. 20, no. 23,
Art. no. 6882, Dec. 2020, doi: [10.3390/s20236882](https://doi.org/10.3390/s20236882).
Luther-condition reference for camera color interpretation; no algorithm copied.
Article licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).

[9] darktable contributors, “Module reference,” *darktable user manual*, ver. 5.6.
[Online]. Available: https://docs.darktable.org/usermanual/5.6/en/module-reference/overview/.
Accessed Oct. 5, 2026. Structure and writing reference for frontend node help.

[10] CIPA and JEITA, “Exchangeable image file format for digital still
cameras: Exif Version 2.32,” CIPA DC-008-2019, May 2019. Exif IFD and tag
definitions for exported capture metadata.

[11] Wayland contributors, “Color management protocol,” *wayland-protocols*,
`color-management-v1.xml`, distributed with `wayland-protocols` Rust crate
0.32.13. [Online]. Available:
https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/color-management/color-management-v1.xml.
Accessed Oct. 5, 2026. Reference-white and surface-description contracts;
MIT license in the protocol source. No implementation copied.

[12] Khronos Group, “Window System Integration (WSI),” *Vulkan Specification*,
“Surface Color Spaces.” [Online]. Available:
https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html.
Accessed Oct. 5, 2026. `VK_COLOR_SPACE_PASS_THROUGH_EXT` permits application-owned
Wayland color management without conflicting with WSI surface ownership.

[13] Yi Cao, “Expose Vulkan passthrough for application-managed Wayland color
descriptions,” gfx-rs/wgpu issue #10545, Oct. 2026. [Online]. Available:
https://github.com/gfx-rs/wgpu/issues/10545.

[14] darktable developers, “Output color profile,” `src/iop/colorout.c`.
[Online]. Available:
https://github.com/darktable-org/darktable/blob/master/src/iop/colorout.c.
Accessed Oct. 5, 2026. Reference for Lab-input proofing and parallel LittleCMS
conversion; no implementation copied. Drip retains its bounded RGB round trip
for softproof instead of modifying the target profile's tone curves.

[16] LibRaw LLC and contributors, “Build configuration,” `configure.ac`,
LibRaw 0.22.2. [Online]. Available:
https://github.com/LibRaw/LibRaw/blob/0.22.2/configure.ac.
Accessed Oct. 7, 2026. Reference for the static build's OpenMP,
JPEG, zlib, LittleCMS and example-program options.

[17] AppImage contributors, “Best practices,” *AppImage documentation*.
[Online]. Available: https://docs.appimage.org/reference/best-practices.html.
Accessed Oct. 7, 2026. Reference for Linux binary build baselines and shared-library
compatibility; does not imply a decision to distribute AppImages.

[18] LibRaw LLC and contributors, “Automake source manifest,” `Makefile.am`,
LibRaw 0.22.2, commit `b93f6e45c194f5df9b02a43b1af9a54b4f41f33f`.
[Online]. Available:
https://github.com/LibRaw/LibRaw/blob/b93f6e45c194f5df9b02a43b1af9a54b4f41f33f/Makefile.am.
The Cargo build reads its shared `lib_libraw_a_SOURCES` declaration directly;
it does not invoke Autotools or maintain a second decoder list.

[15] M. Maria Saguer and Little CMS contributors, “GamutSampler,”
`src/cmsgmt.c`, Little CMS 2.16. [Online]. Available:
https://github.com/mm2/Little-CMS/blob/lcms2.16/src/cmsgmt.c.
Round-trip criterion used to validate gamut warnings; MIT
[license](https://github.com/mm2/Little-CMS/blob/lcms2.16/COPYING).

## CubeCL processing rewrite

CubeCL 0.11.0 is used under MIT OR Apache-2.0; exact dependencies are pinned in
Cargo.lock. [Online]. Available: <https://github.com/tracel-ai/cubecl/tree/v0.11.0>.

The CubeCL Bayer 2x2 and exposure kernels live with their nodes under
`crates/drip/src/node/`; the reusable matrix kernel is in
`crates/drip/src/node/shared_kernel.rs`. They derive from Drip revision
`b9b1045da65376fd5812b7b00362806cde37eaf0` through the validated runtime PoC.
RGB uses packed three-f32 pixels on both host and device. Bayer binning drops incomplete edge cells and averages the two greens.
The same CubeCL source is used by each enabled computation runtime. The original
PoC source and its complete notices are preserved in the external benchmark
archive documented in `agent-docs/CUBECL_REWRITE.md`.

RCD in `crates/drip/src/node/rcd/kernel.rs` and sigmoid's kernel and coefficient
preparation in `crates/drip/src/node/sigmoid.rs` retain the darktable
GPL-3.0-or-later notices. The port is pinned to darktable commit
`61dea294bedb3ab6c7cca1a45530b1ab5c0461f3`, using `src/iop/demosaicing/rcd.c`,
`src/iop/sigmoid.c`, and `src/common/custom_primaries.c` [19].
[Upstream license](https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/LICENSE).
RCD uses full-image intermediate buffers and five dispatches instead of tiled
CPU processing, and a bilinear ten-pixel border. Sigmoid keeps Drip's analytic
0.18-grey coefficients, smooth-preset primary rotations, zero black, and
log-domain curve evaluation. An exponent shift protects finite extreme inputs
in f32. Kernel arithmetic is shared across CubeCL backends; hardware rounding
can change direction ties. Independent upstream C RCD vectors remain in
`crates/drip/tests/reference/`.

[19] darktable developers, “RCD demosaicing, sigmoid and custom primaries,”
source revision `61dea294bedb3ab6c7cca1a45530b1ab5c0461f3`, 2026. [Online].
Available: <https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src>.

Opposed reconstruction in `crates/drip/src/node/highlights/opposed.rs` retains the existing
pinned darktable adaptation and reference vectors. Private CubeCL passes build
three-channel masks, dilate them, reduce chrominance and reconstruct device Bayer
samples; independently supplied clipping levels retain their four site values.
Partial mask cells and clipped edge neighborhoods follow Drip's adaptation.
Cube roots use f32 powers and chrominance uses fixed fan-in-256 f32 reductions
instead of the former f64 row sums; counts remain exact u32. Hardware rounding
can produce small differences. WGPU and the optional CPU backend execute the
same production kernels. The former CPU algorithm remains a test-only reference.

CFA-plane preview reduction in `node/reduce/` restores Drip's same-phase
averaging from revision `b9b1045da65376fd5812b7b00362806cde37eaf0`. It now accepts
an explicit integer factor and retains a minimum Bayer cell for small inputs.
`node/shared_kernel.rs::reduce_bayer` uses f32 accumulation, with exponent scaling to prevent
overflow for large finite samples, and supplies the same computation to the explicit node and demosaic's context-driven local reduction on every
CubeCL backend. Unlike darktable's approximate demosaic path, Drip retains the
selected interpolation algorithm. No preview nodes are injected into the graph.
