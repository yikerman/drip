# Drip

**Done Right Image Processing**

Your photos. Your workflow.

Drip is an open-source RAW photo editor that puts the whole processing pipeline
in your hands. Start with a ready-to-edit workflow, explore your image at every
step, and make the workspace your own.

![Sony A7R III photo in Drip's complete RAW-to-TIFF pipeline, with preview and scopes](fixtures/demos/pipeline-overview.png)

*From RAW to TIFF: your photo, processing steps and live scopes on one canvas.*

### Put your GPU to work

CubeCL powers Drip's image processing, with the same kernels across Vulkan,
Metal, CUDA and other compute backends. An optional CPU backend runs those
same kernels, too. Adjust preview detail as you edit; export at full resolution.

### Give your photo more room

Keep your image front and center, your scopes alongside it, and your favorite
controls within reach. Previews, scopes and adjustment panels can live on the
canvas or in their own windows. Arrange them your way.

![Detached preview with cyan gamut warnings, waveform, sigmoid and exposure controls](fixtures/demos/pop-out-workspace.png)

*A workspace built around the photo, with detached waveform and tone controls.
Cyan highlights show colors outside the selected proof profile's gamut.*

### Keep color in view

Wide-gamut previews, ICC softproofing and color-managed TIFF export carry your
edit from screen to file. Underneath, floating-point linear Rec.2020 processing
and CAT16 white balance keep color handling explicit. Histograms, waveforms and
vectorscopes let you see what each adjustment does.

![Histogram, waveform and vectorscope on the canvas, with histogram settings in the inspector](fixtures/demos/scopes.png)

*A closer look at brightness and color: histogram, waveform and vectorscope,
placed wherever you need them in the pipeline.*

### Experiment with confidence

Branch the pipeline, try another processing order, or inspect an intermediate
result. Drip checks image types and color spaces when you connect nodes, then
checks image-dependent requirements during processing. Incompatible operations
explain what needs attention; valid combinations leave room for creative choices.

![RAW processing connections with chromatic adaptation selected and its help and white-balance control visible](fixtures/demos/sane-workflow.png)

*Follow the image from sensor data through demosaic and camera color, with
white balance and its assumptions open in the inspector.*

### Make your next edit feel familiar

Save a pipeline as a reusable template, with the settings you want to change
brought to the surface. Keep your starting look ready for the next photo.

![Exposure's context menu offers Expose as template parameter, alongside the reusable RAW-to-TIFF pipeline](fixtures/demos/generalized-automation.png)

*Turn an adjustment into a template parameter—here, Exposure—so your reusable
workflow presents the controls you choose.*

## Build and run

Drip is an early **0.1.0-dev** prototype for Bayer RAW photos, developed on Linux
with Wayland. Windows and macOS builds and display behavior still need verification.

Install [Rust](https://rustup.rs/) 1.95 or newer and your platform's build tools.
LibRaw, JPEG, zlib and LittleCMS are built from pinned sources; system installs
of those libraries are unnecessary.

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
and put them on `PATH`. Run the build below in Developer PowerShell.

</details>

From the checkout:

```sh
git submodule update --init --recursive
git lfs install
git lfs pull
cargo xtask dist
./dist/drip-gui
```

On Windows, run `dist\drip-gui.exe`. The distribution build selects Vulkan on
Linux, native Metal on macOS, and D3D12 on Windows. See
[compute backends](crates/drip/README.md#compute-backends) for alternatives.

Choose the RAW file under **Template parameters** to begin. You can also pass
an existing `.drip` project or `.drip-template` to `drip-gui` when launching it.

## Canvas gestures

The canvas uses the left button to navigate and the right button to edit:

| Gesture | Action |
| --- | --- |
| Left-drag / scroll | Pan / zoom around the pointer |
| Right-drag a node | Move it |
| Right-click empty canvas / a node | Add / rename or delete |
| Right-click a port, then another | Connect; either direction works |
| Right-click a wire | Disconnect |
| Escape / right-click empty canvas while connecting | Cancel the pending wire |
| Left-click a port | Open the connected-node navigation menu |

## Status and development

Undo/redo, denoising, lens corrections and batch processing are still to come.
Project formats may change during development. Display color has software
validation; instrument-based verification is still pending.

See the [roadmap](agent-docs/TODO.md), [architecture and backend guide](crates/drip/README.md),
[design decisions](agent-docs/DESIGN.md) and [validation notes](agent-docs/PROGRESS.md).
Algorithm sources and dependency credits are in [THIRD_PARTY.md](THIRD_PARTY.md).

## License

AGPL-3.0-or-later; see [LICENSE](LICENSE).
