# Third-party credits

Drip is AGPL-3.0-or-later. We thank the authors and contributors of the projects
below. Dependency versions are recorded in `Cargo.lock`; native library versions
come from the build environment. This lists direct dependencies and major native
components, rather than duplicating the full transitive dependency tree.

## Processing sources

Adapted code retains its upstream copyright and license notices. References
beside each algorithm identify the source and explain changes made for Drip.

| Source and credit | Use | License |
|-------------------|-----|---------|
| [darktable developers](https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3), including sigmoid and custom-primaries contributors | Sigmoid curve, hue/energy correction and primaries handling, adapted to Rust/Rayon | GPL-3.0-or-later; [license text](https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/LICENSE) |
| [Luis Sanz Rodríguez](https://github.com/LuisSR/RCD-Demosaicing), Ingo Weyrich, Hanno Schwalm and darktable contributors | RCD demosaicing, adapted from the pinned darktable revision above; bilinear border in Drip | Original RCD: GPL-3.0; darktable integration: GPL-3.0-or-later |
| garagecoder and Iain (G’MIC), Hanno Schwalm and [darktable contributors](https://github.com/darktable-org/darktable/tree/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/hlreconstruct) | Inpaint-opposed highlight reconstruction; Bayer adaptation with complete edge neighborhoods | GPL-3.0-or-later |
| [Dave Coffin, dcraw](https://www.dechifro.org/dcraw/) | Reference for camera matrix normalization and RAW black-level conventions, through LibRaw | dcraw's source contains multiple licensing options; credited here as an algorithm reference, not a vendored dependency |

Further algorithm ports must add their authors, exact source revision and license
here when introduced.

## Runtime dependencies

| Project and credit | Use | License |
|--------------------|-----|---------|
| [LibRaw LLC and contributors](https://www.libraw.org/) | RAW decoding through Drip's C shim | LGPL-2.1 OR CDDL-1.0 |
| [Marti Maria and Little CMS contributors](https://github.com/mm2/Little-CMS) | ICC color transforms | MIT |
| [Kornel Lesiński and rust-lcms2 contributors](https://github.com/kornelski/rust-lcms2) (`lcms2`, `lcms2-sys`) | Rust Little CMS bindings | MIT |
| [Rayon contributors](https://github.com/rayon-rs/rayon) | CPU parallelism | MIT OR Apache-2.0 |
| [image-rs contributors](https://github.com/image-rs/image-tiff) (`tiff`) | TIFF encoding | MIT |
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
| [cc-rs contributors](https://github.com/rust-lang/cc-rs) (`cc`) | Compile the LibRaw shim | MIT OR Apache-2.0 |
| [pkg-config-rs contributors](https://github.com/rust-lang/pkg-config-rs) | Find native libraries | MIT OR Apache-2.0 |
| [vcpkg-rs contributors](https://github.com/mcgoo/vcpkg-rs) | Find LibRaw on Windows | MIT OR Apache-2.0 |
| [egui contributors](https://github.com/emilk/egui) (`egui_kittest`) | Headless GUI tests | MIT OR Apache-2.0 |

RAW fixture provenance is recorded with the files in `fixtures/`.

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
Luther-condition reference for image capability contracts; no algorithm copied.
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
