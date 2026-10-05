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
| [gfx-rs contributors](https://github.com/gfx-rs/wgpu) (`wgpu`) | GPU rendering | MIT OR Apache-2.0 |
| [rust-windowing contributors](https://github.com/rust-windowing/winit) (`winit`) | Windows and input events | Apache-2.0 |
| [Smithay contributors](https://github.com/Smithay/wayland-rs) (`wayland-client`, `wayland-protocols`) | Wayland pop-out parenting | MIT |
| [PolyMeilex and rfd contributors](https://github.com/PolyMeilex/rfd) | Native file dialogs | MIT |
| [Serde contributors](https://github.com/serde-rs/serde) (`serde`, `serde_json`) | Project serialization | MIT OR Apache-2.0 |
| [David Tolnay and thiserror contributors](https://github.com/dtolnay/thiserror) | Error definitions | MIT OR Apache-2.0 |
| [Rust log contributors](https://github.com/rust-lang/log), [env_logger contributors](https://github.com/rust-cli/env_logger) | Logging | MIT OR Apache-2.0 |
| [half-rs contributors](https://github.com/VoidStarKat/half-rs) (`half`) | Half-float texture preparation | MIT OR Apache-2.0 |
| [Joshua Barretto and pollster contributors](https://github.com/zesterer/pollster) | Blocking GPU initialization | MIT OR Apache-2.0 |

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
