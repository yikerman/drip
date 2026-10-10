# Drip

A Rust RAW processor with a checked node graph and CubeCL image kernels.

Nodes declare concrete payloads, CPU/device placement, and interpretation
contracts. Graph edits validate contracts before committing. Evaluation runs
the requested dependency subgraph and owns transfers between host and device.
Intermediate image results are not cached across evaluations.

The current RAW path is Bayer normalization, independent as-shot gains and
camera matrix outputs, opposed highlight reconstruction, RCD demosaic, matrix
conversion, exposure, and sigmoid.
The GUI retains color-managed wgpu presentation, ICC proofing, diagnostic scopes,
and TIFF export. Current decisions and validation are recorded in
[Design](agent-docs/DESIGN.md) and [Progress](agent-docs/PROGRESS.md).

## Build & install

Drip is an early **0.1.0-dev** prototype for Bayer RAW photos, developed on Linux
with Wayland. You'll need to build it from source; Windows and macOS packaging
hasn't been verified yet.

Install [Rust](https://rustup.rs/) 1.95 or newer, then the build tools for your
platform. Cargo builds pinned LibRaw, JPEG, zlib and LittleCMS sources statically;
system installations of those libraries are unnecessary. Linux uses the system
C++ runtime (`libstdc++` with GCC).

<details>
<summary>Fedora</summary>

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
<summary>macOS (Homebrew)</summary>

Install the Xcode Command Line Tools and [Homebrew](https://brew.sh/), then:

```sh
xcode-select --install
brew install cmake nasm git-lfs
```

</details>

<details>
<summary>Windows x64 (MSVC)</summary>

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

Without `--target`, this builds for the Rust compiler's host. Distribution builds
select Vulkan/SPIR-V on Linux, native Metal on macOS, and WGSL/D3D12 on Windows.
These are also their startup defaults. WGSL remains available through
`DRIP_BACKEND=wgsl`; CPU/LLVM, CUDA and HIP are not included by `xtask dist`.

## Usage

Launch the installed editor with `drip-gui`, or run it from the checkout:

```sh
cargo run --release -p drip-gui
```

Choose a RAW file under **Template parameters** to start editing. Select a node to adjust it,
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
alphabetically. Choose, for example, **Tone → Exposure**. New nodes use the
declared operation name; **Rename** edits that node's name.

Scroll to zoom around the pointer. A port's navigation menu lists connected
`node · port` entries; choose one to select that node and bring it into view.
Opening the menu preserves selection. Canvas ports show the port name and declared payload type; `?` marks an optional input.
Hover over a port for its full contract, or a truncated node title for its full name.

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
in pop-outs and template parameters. The parameter name's menu also offers reset.

In **Preview**, set **Proof mode** to **None** for the original image,
**Softproof** to simulate an output profile, or **Gamut check** to mark
out-of-gamut colors in cyan. The active proof mode contains its profile,
rendering intent and black point compensation settings. Match them to
**Export TIFF**. Choose **File** under **Output profile** to select an ICC file.
These modes affect only the preview and its pop-out.

Gamut check is approximate near the gamut boundary.

Histogram and waveform axes show exposure in stops relative to sample value 1.
**Density scale** changes counts, not the exposure axis. Scopes identify their source color space; other settings remain in the
parameter panel and global detail control.
Camera scopes plot native channels without a color conversion.

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

Set **Output file** and press **Export** in **Export TIFF**. Export always
uses full processing detail. **Deflate** compression exposes its compression
level; **None** writes an uncompressed TIFF.

Save your work as a `.drip` project, or use **Save template…** to write a
`.drip-template`. Opening a template creates an untitled project; **Save** asks
for a project destination. The title bar shows the current filename and an
asterisk after unsaved changes.

Right-click a parameter name and choose **Expose as template parameter** to
include it under **Template parameters**. Saving a template resets exposed
parameters to their node-defined defaults; **Keep value in template** retains
the current value instead. An exposed enum includes its selected variant and
that variant's settings.

You can also open a saved project from the command line:

```sh
cargo run --release -p drip-gui -- project.drip
```

Saved projects use format version 2 and repeat connection checks when opened.
Node IDs use `category.operation`; this revision also nests variant-specific
parameters in their enum values. Older IDs and parameter layouts are rejected
without migration or modification. RAW files are bound as
immutable decoded snapshots; reopening or invalidating reloads them.
Preview detail is passed in the global evaluation context. Demosaic reduces its
Bayer phase planes internally before interpolation, after upstream sensor
corrections. Every contract and node receives the same context; no hidden nodes
or temporary graph rewrites are involved. Export requests full detail.


For headless TIFF export:

```sh
cargo run --release -p drip-cli -- photo.nef output.tiff
cargo run --release -p drip-cli -- project.drip output.tiff
```

The CLI uses default output-profile settings and 16-bit TIFF. A GUI project
needs one connected export image to supply an unambiguous batch target;
frontend view nodes outside that target's dependencies are not loaded.

`DRIP_BACKEND` selects computation for both frontends, including GUI previews
and exports. Without an override it prefers native Metal on macOS when built
with `drip/metal-native`, or Vulkan on Linux with `drip/vulkan`; otherwise it uses
`wgpu`. An ordinary default-feature Cargo build still uses wgpu. To run the same
kernels on CPU:

```sh
DRIP_BACKEND=cpu cargo run --release -p drip-gui --features drip/cpu -- project.drip
DRIP_BACKEND=cpu cargo run --release -p drip-cli --features drip/cpu -- photo.nef output.tiff
```

`DRIP_BACKEND=cuda` requires `--features drip/cuda`. Unknown values or backends
not enabled in the build report an error. An explicit selection is never replaced
by another backend, and failed initialization does not trigger a fallback. GUI
drawing still uses wgpu. See [compute backend options](crates/drip/README.md#compute-backends).

`RUST_LOG=warn,drip_gui::worker=trace` prints per-node host timings for evaluation
and GUI preparation, followed by the total preview time. Tracing adds no device
waits; asynchronous GPU execution is not measured separately. Evaluation timings
include input transfers, contracts and dispatch; existing blocking readbacks are
charged to their consumer. The final runtime wait appears only in the preview
total. Timing records are printed after processing; clock reads and record storage
still add a small cost. Use `drip_gui::worker=debug` for preview totals alone.

## Development

```sh
DRIP_BACKEND=cpu cargo test --workspace --features drip/cpu
cargo test -p drip --test algorithms -- --ignored
cargo test -p drip --test model cubecl_wgpu_hybrid -- --ignored
cargo clippy --workspace --all-targets --no-deps -- -D warnings
```

The ignored tests require a compute-capable GPU. `drip/cpu` uses CubeCL's CPU
runtime; `drip/cuda` enables CUDA. All use the same image kernel sources.
Presentation shaders remain WGSL in `drip-gui`. GUI preparation consumes host RGB values. Device-side scope preparation and
GPU resource sharing remain follow-up work.

The macro crate separates parsing/validation from binding generation.
Node functions own Rustdoc help and contracts; the macro generates port handles,
discovery, parameter persistence and invocation adapters.
Parameter schemas come from the function's parameter type via `Parameters`;
derive it on settings structs and use `&()` for nodes without parameters.

Parameter and choice labels default to sentence case; `#[label("sRGB")]`
overrides display text without renaming the persisted key. A `Choice` variant
may be empty or contain one struct deriving `Parameters` and Serde traits:

```rust
#[derive(drip::Choice)]
enum Proof {
    #[choice("none")]
    None,
    #[choice("softproof")]
    Softproof(drip::node::profile::Settings),
}
```

The generated schema renders only the selected variant's fields. Serde uses
`"none"` for an empty variant and `{"softproof": {...}}` for a variant with
settings. File-dialog hints belong to the node's GUI binding and apply in the
inspector, parameter pop-out and template panel.

Port labels retain declared payload names. The node macro's optional
`port_labels(port = "PayloadName")` overrides display text without changing
connection names or compatibility. Unknown ports and duplicate labels are
compile errors. RAW sources currently implement `Node` directly to bind assets
before evaluation and return shared buffers; the function macro handles
parameter-only nodes with allocated outputs.

Borrowed algorithms and dependency licenses are in [THIRD_PARTY.md](THIRD_PARTY.md).
Benchmark outputs and retired experiment sources live outside this repository;
the archive location is recorded in the rewrite status.

## License

AGPL-3.0-or-later; see [LICENSE](LICENSE).

Source ownership rules are in [crates/drip/README.md](crates/drip/README.md).
