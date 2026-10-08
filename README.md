# Drip

**Done Right Image Processing**

Drip is an open-source RAW photo editor that puts the processing pipeline in
your hands. Develop a photo, follow it through each adjustment, and build a
workflow you can use again.

![Drip showing a photo preview, histogram, waveform and vectorscope alongside separate sigmoid and exposure controls](fixtures/demos/pipeline-and-scopes.png)

## See what goes into your image

From white balance to the final export, every step is there on the canvas.
Connect processing nodes, branch the pipeline, and place a preview wherever you
want a closer look. Keep a histogram, waveform or vectorscope alongside your
photo to see how your adjustments affect it.

Start with the included RAW-to-TIFF workflow and make it your own:

```text
RAW → white balance → highlight reconstruction → demosaic
    → camera color → exposure → sigmoid → TIFF
```

![The complete RAW-to-TIFF node graph with preview, histogram, waveform and vectorscope](fixtures/demos/pipeline-overview.png)

## Make room for the way you edit

Give your photo more space. Keep exposure and contrast controls close by. Move
a scope into its own window, or keep everything together on the canvas.
Previews, scopes and parameter panels can be arranged independently, so you
choose what stays in view.

![Drip with separate photo preview, histogram and vectorscope windows above the processing graph](fixtures/demos/pop-out-workspace.png)

Adjustments update in the background. Lower the preview detail while exploring,
then turn it up for a closer look. Exports always use full resolution.

When you've built a pipeline you like, save it as a template for the next photo.
Choose which settings to expose as inputs, and keep the rest ready to go.

## Keep color in the picture

Drip supports wide-gamut previews and ICC-managed TIFF export, with
floating-point processing in linear Rec.2020. Color handling is part of the
workflow from camera conversion through display and export.

## Build & install

Drip is an early **0.1.0-dev** prototype for Bayer RAW photos, developed on Linux
with Wayland. You'll need to build it from source; Windows and macOS packaging
hasn't been verified yet.

Install [Rust](https://rustup.rs/) 1.95 or newer, then the build tools for your
platform. Cargo builds pinned LibRaw, JPEG, zlib and LittleCMS sources statically;
system installations of those libraries are unnecessary. Linux uses the system
C++ runtime (`libstdc++` with GCC).

Processing uses FP32 WGSL kernels through wgpu. A supported GPU driver is enough;
CUDA libraries are not required. A software Vulkan adapter such as Mesa lavapipe
runs the same kernels when hardware is unavailable. Whole-image allocations must
fit the adapter's storage-buffer limit and the compute budget (2 GiB by default);
large-image tiling is not implemented yet.

<details>
<summary>Fedora (Tier 0 Support!)</summary>

```sh
sudo dnf install gcc-c++ cmake make nasm pkgconf-pkg-config wayland-devel libxkbcommon-devel git-lfs
```

</details>

<details>
<summary>Debian / Ubuntu</summary>

```sh
sudo apt update
sudo apt install build-essential cmake nasm pkg-config libwayland-dev libxkbcommon-dev git-lfs
```

Use rustup if your distribution's Rust version is too old.

</details>

<details>
<summary>macOS (Homebrew; Tier 1 support)</summary>

Install the Xcode Command Line Tools and [Homebrew](https://brew.sh/), then:

```sh
xcode-select --install
brew install cmake nasm git-lfs
```

</details>

<details>
<summary>Windows x64 (here be dragons)</summary>

Install Git with Git LFS, Rust's MSVC toolchain, and Visual Studio Build Tools
with **Desktop development with C++** and a Windows SDK. Install CMake and NASM
and put them on `PATH`. Run the build below in Developer PowerShell. Native
compilation follows Rust's selected CRT linkage. Windows runtime packaging and
execution remain unverified.

</details>

From the Drip checkout, initialize the release-pinned submodules, fetch the
fixtures and build the editor:

```sh
git submodule update --init --recursive
git lfs pull
cargo build --release --workspace
```

Build both frontends and stage only their executables in `./dist/`:

```sh
cargo xtask dist
# Or select a target explicitly:
cargo xtask dist --target x86_64-unknown-linux-gnu
```

## Usage

Launch the installed editor with `drip-gui`, or run it from the checkout:

```sh
cargo run --release -p drip-gui
```

Choose a RAW file under **inputs** to start editing. Select a node to adjust it,
or use its gear button to open the controls in a separate window. The pop-out
button on previews and scopes opens their views separately.

The inspector describes the selected node's operation, assumptions and input/output types.

Canvas gestures separate **navigation on the left button** from **modification
on the right button**. Exploring the graph should preserve its nodes and wiring.
Moving a node changes its layout, so it uses the right button too.

| Context | Left click | Left drag | Right click | Right drag |
|---------|------------|-----------|-------------|------------|
| Empty canvas | Clear selection | Pan canvas | Add node menu | No action |
| Node body | Select and show inspector | Pan canvas | Rename / Delete menu | Move node |
| Input port | Show source menu | Pan canvas | Start or complete a connection | No action |
| Output port | Show destinations menu | Pan canvas | Start or complete a connection | No action |
| Wire | Clear selection | Pan canvas | Disconnect highlighted wire | No action |

The Add node menu groups kinds by category, with categories and node names sorted
alphabetically. Choose, for example, **color → Exposure**. New nodes use the
capitalized default name; **Rename** edits that node's name.

Scroll to zoom around the pointer. A port's navigation menu lists connected
`node · port` entries; choose one to select that node and bring it into view.
Opening the menu preserves selection. Hover over a port to see its type.

Buttons, menu items and parameter controls use ordinary left-click operation;
drag a view's corner handle to resize it.

In parameter panels, scroll over a slider to adjust it: up increases, down
decreases. Each scroll line changes a floating-point value by 1% of its range,
or an integer by one. The panel stays still while scrolling over a slider.

Scroll over a dropdown to change its selection: down selects the next option,
up the previous, stopping at either end. This also works for **Preview detail**
and the preview pop-out's zoom selector.

Right-click a slider, checkbox, dropdown or filename and choose **Reset to default**
to restore its node-defined default (or clear a path). These gestures also work
in pop-outs and template inputs. The parameter name's menu also offers reset.

In **Preview**, choose `none`, `softproof` to simulate the selected profile and
intent, or `gamutcheck` to mark out-of-gamut colors in cyan. Match the profile,
intent and black-point compensation to Export. These modes affect only the
preview and its pop-out.

Gamutcheck is an approximate warning near the gamut boundary.

In the preview pop-out, scroll to zoom around the pointer and left-drag to pan.
The zoom selector offers **Fit**, **25%**, **50%**, **100%**, **200%** and **400%**.
Fit follows the window size; fixed percentages account for desktop scaling.
At 100%, one rendered image pixel occupies one display pixel. The dimensions and
detail label describe that rendered image; choose **Full** Preview detail for
full-resolution inspection.

Node bodies gain a thin border on hover. Wires thicken when targeted; their hit
band stays narrow at every zoom level. Nodes, ports and controls take priority
over wires. At a crossing, the nearest wire is highlighted and right-click removes
only that connection.

To connect nodes, right-click a port, move to a compatible port in the opposite
direction, then right-click again. You can start at either an input or an output.

An input keeps its current source until a valid replacement is committed; an
output can feed multiple inputs. Invalid targets leave the connection pending
and report the reason.

**Escape** or **right-click on empty canvas** cancels the
temporary wire without changing existing connections or opening the Add node menu.

Set an output path and press **export** in the export node's panel when you're
ready.

Save your work as a `.drip` project, or use **Save template** to reuse the
pipeline. Right-click a parameter name to make it a template input.

You can also open a saved project from the command line:

```sh
cargo run --release -p drip-gui -- project.drip
```

## Under the hood

Drip is written in Rust, with Rayon for processing and wgpu/egui for the GUI.
The processing library is independent of the interface. Local macros derive DAG
contracts and help from typed node functions. Concrete ports distinguish sensor
data, camera RGB and working Rec.2020 RGB; creative processing does not impose
an ordering restriction. Several algorithms
come from darktable; sources and credits are in [THIRD_PARTY.md](THIRD_PARTY.md).

For development, see the [library design and reading guide](crates/drip/src/lib.rs)
and [GUI overview](crates/drip-gui/src/main.rs).
Build its linked API docs with `cargo doc -p drip --no-deps`.

```sh
cargo test --workspace --all-targets
cargo test --workspace --doc
```

Runnable declaration/DAG examples:

```sh
cargo run -p drip --example calibration_dag
cargo run -p drip --example masked_edit_dag
```

Logs go to stderr: `RUST_LOG=warn,drip=debug` for diagnostics,
`RUST_LOG=warn,drip_gui::frame=trace` for frame timings.

## License

AGPL-3.0-or-later; see [LICENSE](LICENSE).
RUST_LOG=warn,drip_gui::frame=trace

## Library example

Run the complete RAW pipeline with resident GPU intermediates and an explicit
CPU consumer, without opening a window:

```sh
cargo run --release -p drip --example gpu_pipeline -- photo.arw 2
```

The final argument selects preview scale: `0` is full resolution, `2` is one
quarter along each axis. The demo checks finite output and reports image transfers.
The node examples `calibration_dag` and `masked_edit_dag` show local declarations,
runtime interpretation checks and a mixed CPU/GPU graph.
