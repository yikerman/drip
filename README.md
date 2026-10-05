# Drip

**Done Right Image Processing**: a node-based raw photo processor in Rust,
built around color accuracy. A processing pipeline is a typed graph of nodes,
built and tuned interactively in a GUI and reusable as a template for batch
processing.

Status: early prototype. Linux on Wayland is the primary platform; Windows and
macOS packaging is not yet verified.

## Goals

- **A graph you can trust.** Typed connections refuse mismatches, such as
  exporting an image that has not been tone mapped. The GUI evaluates every
  node regardless of canvas position; a node is recomputed only when something
  it depends on changes.
- **Color done right.** Processing runs in 32-bit float and linear Rec.2020.
  Exports are TIFFs with an embedded ICC profile. On Wayland, previews keep
  their wide gamut and the compositor maps them to the display.
- **Templates as functions.** A template is a graph whose parameters can be
  bound to named inputs (such as the raw file and the output path). A project
  is a template applied to values for those inputs.

## How it works

| Crate | Role |
|-------|------|
| `drip` | The library: node graph, evaluation, color management, persistence. No GUI dependency. |
| `drip-libraw` | Minimal binding to [LibRaw](https://www.libraw.org/): a C shim behind one safe `decode()`. |
| `drip-gui` | Node editor and preview: winit + wgpu + egui. |
| `drip-cli` | Batch frontend (not implemented yet). |

The prototype pipeline is:

raw → white balance → 2×2 binning debayer → camera to Rec.2020 → exposure → sigmoid tone mapping → TIFF export (sRGB, Display P3, Rec.2020 or any RGB ICC profile)

Preview and histogram nodes can be attached anywhere in the graph.

[LittleCMS](https://www.littlecms.com/) does the color transforms.
`docs/DESIGN.md` records current intent and tradeoffs;
`docs/PROGRESS.md` is a compact development handoff.

## Usage

Requirements: Rust 1.92 or newer, LibRaw (with `libraw_r`), LittleCMS 2,
pkg-config, and Git LFS for the test fixtures.

```sh
# Fedora
sudo dnf install LibRaw-devel lcms2-devel git-lfs
git lfs install && git lfs pull

# The GUI, starting from the built-in raw-to-TIFF template or a project file
cargo run -p drip-gui -- [project.drip]

# One raw file through the built-in template, without the GUI
cargo run -p drip --example raw_to_tiff -- photo.arw srgb out.tif

cargo test --workspace
```

In the GUI:
- Right-click the editor background to add a node.
- Drag from an output port to an input port to connect them; right-click an
  input port to disconnect it.
- Choose **Preview detail** globally: Full through 1/256, default 1/2. The
  selection is saved with the project; exports always use full detail.
- Previews update in the background. The status line shows **evaluating…**
  while the previous completed image remains visible.
- Select a node to edit its parameters. Right-click a parameter name to bind
  it to a graph input.
- Set the raw file and the output path under *inputs*, then press *export*
  on the export node.

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

## License

AGPL-3.0-or-later; see `LICENSE`. Dependencies and adapted algorithms are credited
in [THIRD_PARTY.md](THIRD_PARTY.md).
