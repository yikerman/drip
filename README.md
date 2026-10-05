# Drip

**Done Right Image Processing**: a node-based raw photo processor in Rust,
built around color accuracy. A processing pipeline is a typed graph of nodes,
built and tuned interactively in a GUI and reusable as a template for batch
processing.

Status: **0.1.0-dev**, an early prototype for Bayer RAW photos. Linux on Wayland
is the primary platform; Windows and macOS packaging is not yet verified.

## Goals

- **A graph you can trust.** Typed connections refuse mismatches, such as
  exporting an image that has not been tone mapped. The GUI evaluates every
  node regardless of canvas position; a node is recomputed only when something
  it depends on changes.
- **Color done right.** Processing runs in 32-bit float and linear Rec.2020.
  Exports are TIFFs with an embedded ICC profile. Wide-gamut previews use scRGB
  and compositor color management, with a visible warning on sRGB fallback.
- **Templates as functions.** A template is a graph whose parameters can be
  exposed as per-node inputs, such as the raw file and output path. Saving a
  template resets those inputs to their defaults; a project keeps their values.

## How it works

| Crate | Role |
|-------|------|
| `drip` | The library: node graph, evaluation, color management, persistence. No GUI dependency. |
| `drip-libraw` | Minimal binding to [LibRaw](https://www.libraw.org/): a C shim behind one safe `decode()`. |
| `drip-gui` | Node editor and preview: winit + wgpu + egui. |
| `drip-cli` | Batch frontend (not implemented yet). |

The default pipeline is:

```text
RAW normalization → as-shot white balance → inpaint-opposed highlights
→ RCD demosaic → linear Rec.2020 → exposure → sigmoid → ICC TIFF export
```

Sigmoid, RCD and highlight reconstruction are adapted from darktable into Rust
with Rayon parallelism. RCD produces full-size output; the older half-size
binning node remains available. Sensor processing runs at full resolution,
then preview reduction happens immediately before demosaicing. Export always
uses full detail.

Exposure adjusts scene brightness independently of sigmoid's contrast, skew and
hue preservation. Sigmoid's panel plots the same curve used for processing.
Exports support 16-bit integer or float TIFF, with sRGB, Display P3, Rec.2020 or
a suitable custom RGB ICC profile. Preview nodes accept scene/display Rec.2020;
histograms and RGB waveforms also accept camera RGB. Add waveform or vectorscope
nodes from the node menu to inspect exposure across image columns or D65-centered
u′v′ chromaticity. Waveform levels use EV; vectorscope markers show Rec.2020
primaries. Black has no chromaticity, and negative channels are clipped for the
vectorscope visualization only.

[LittleCMS](https://www.littlecms.com/) does the color transforms.
Backend nodes keep their schemas and algorithms together. Frontend node UIs
reuse schema controls and a shared editing interface; custom UI is optional.
[DESIGN](docs/DESIGN.md) records intent and tradeoffs;
[PROGRESS](docs/PROGRESS.md) is a compact development handoff.

## Usage

Requirements: Rust 1.92 or newer, LibRaw (with `libraw_r`), LittleCMS 2,
pkg-config, and Git LFS for the test fixtures.

```sh
# Fedora
sudo dnf install LibRaw-devel lcms2-devel git-lfs
git lfs install && git lfs pull

# The GUI, starting from the built-in raw-to-TIFF template or a project file
cargo run --release -p drip-gui
# Or open a saved project:
cargo run --release -p drip-gui -- project.drip

# One raw file through the built-in template, without the GUI
cargo run --release -p drip --example raw_to_tiff -- photo.arw srgb out.tif

cargo test --workspace
```

In the GUI:

- Drag empty canvas to pan and scroll to zoom. Right-click the background to add a node.
- Drag from an output port to an input port to connect them; right-click an
  input port to disconnect it.
- Choose **Preview detail** globally: Full through 1/256, default 1/2. The
  selection is saved with the project; exports always use full detail.
- Previews update in the background. The status line shows **evaluating…**
  while the previous completed image remains visible.
- Select a node to edit its parameters. Use **⚙** to open its parameter window,
  or **🗗** on a preview or scope to open its view separately. Closing the view
  window returns it to the canvas; pop-out state is not saved.
- Right-click a parameter name to make it a template input or fix its value.
- Set the raw file and the output path under *inputs*, then press *export*
  in the export node’s parameter panel.
- Use **Invalidate cache** after changing an input file on disk. Save projects
  and templates as `.drip`; the suffix is appended when missing.

Set `RUST_LOG=debug` to see evaluation timings.

Pixel-processing nodes use ordinary Rust kernels with Rayon CPU parallelism.
Each kernel has one implementation. Set `RAYON_NUM_THREADS=1` to use one worker,
or another positive count to limit the pool; the default uses available CPU
parallelism. GPU computation is postponed. The GUI still uses wgpu for drawing.

To measure complete preview evaluation without drawing:

```sh
RAYON_NUM_THREADS=12 cargo run --release -p drip --example preview_latency -- photo.arw
```

See [the compute measurements](docs/DESIGN.md#performance-evidence)
for historical hardware measurements and their limits.

## Before v0.1

The pipeline has numerical reference tests, upstream C comparisons and a real
Sony RAW-to-TIFF test. Broader photographic validation comes next, followed by
UI/UX refinement, SpyderX display verification, and packaging. Wide-gamut output
has not yet been verified with the colorimeter.

Project files must match the current schema; prototype changes can break older
files. Batch CLI, undo/redo, denoising and lens corrections are not implemented.
See [TODO](TODO.md) for remaining work and deferred items.

## License

AGPL-3.0-or-later; see `LICENSE`. Dependencies and adapted algorithms are credited
in [THIRD_PARTY.md](THIRD_PARTY.md).
