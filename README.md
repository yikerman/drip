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

## Try it

Drip is an early **0.1.0-dev** prototype for Bayer RAW photos, developed on Linux
with Wayland. You'll need to build it from source; Windows and macOS packaging
hasn't been verified yet.

Install [Rust](https://rustup.rs/) 1.92 or newer, then the dependencies for your
platform:

<details>
<summary>Fedora (Tier 0 Support!)</summary>

```sh
sudo dnf install gcc-c++ pkgconf-pkg-config LibRaw-devel lcms2-devel wayland-devel libxkbcommon-devel git-lfs
```

</details>

<details>
<summary>Debian / Ubuntu</summary>

```sh
sudo apt update
sudo apt install build-essential pkg-config libraw-dev liblcms2-dev libwayland-dev libxkbcommon-dev git-lfs
```

Use rustup if your distribution's Rust version is too old.

</details>

<details>
<summary>macOS (Homebrew; Tier 1 support)</summary>

Install the Xcode Command Line Tools and [Homebrew](https://brew.sh/), then:

```sh
xcode-select --install
brew install libraw little-cms2 pkgconf git-lfs
```

</details>

<details>
<summary>Windows x64 (here be dragons)</summary>

Install Git with Git LFS, Rust's MSVC toolchain, and Visual Studio Build Tools
with **Desktop development with C++** and a Windows SDK. In Developer PowerShell,
set up [vcpkg](https://learn.microsoft.com/en-us/vcpkg/get_started/get-started)
and the native libraries:

```powershell
$env:VCPKG_ROOT = "$env:USERPROFILE\vcpkg"
git clone https://github.com/microsoft/vcpkg.git $env:VCPKG_ROOT
& "$env:VCPKG_ROOT\bootstrap-vcpkg.bat"
$env:VCPKGRS_TRIPLET = "x64-windows-static-md"
& "$env:VCPKG_ROOT\vcpkg.exe" install libraw:x64-windows-static-md lcms:x64-windows-static-md
$env:LCMS2_LIB_DIR = "$env:VCPKG_ROOT\installed\x64-windows-static-md\lib"
```

Run the build below from the Drip checkout in that same terminal. These steps
match the build scripts but haven't been tested on Windows.

</details>

From the Drip checkout, fetch the fixtures and start the editor:

```sh
git lfs install
git lfs pull
cargo run --release -p drip-gui
```

Choose a RAW file under **inputs** to start editing. Select a node to adjust it,
or use its gear button to open the controls in a separate window. The pop-out
button on previews and scopes opens their views separately.

Drag the canvas to pan, scroll to zoom, and right-click to add nodes. Connect
them by dragging between ports. Right-click an input port to disconnect it.

Set an output path and press **export** in the export node's panel when you're
ready. Save your work as a `.drip` project, or use **Save template** to reuse the
pipeline. Right-click a parameter name to make it a template input.

You can also open a saved project from the command line:

```sh
cargo run --release -p drip-gui -- project.drip
```

## What's next

The next steps are testing on more photographs, refining the editing experience,
checking display color with a SpyderX, and packaging the first release.
Instrument-based wide-gamut verification is still outstanding.

Batch processing, undo/redo, denoising and lens corrections are still to come.
Project files may change as the prototype develops. See [TODO](TODO.md) for
what's planned.

## Under the hood

Drip is written in Rust, with Rayon for processing and wgpu/egui for the GUI.
The processing library is independent of the interface. Several algorithms
come from darktable; sources and credits are in [THIRD_PARTY.md](THIRD_PARTY.md).

For development, see the [library design and reading guide](crates/drip/src/lib.rs)
and [GUI overview](crates/drip-gui/src/main.rs).
Build its linked API docs with `cargo doc -p drip --no-deps`.
See [DESIGN](docs/DESIGN.md) for decisions and
[PROGRESS](docs/PROGRESS.md) for the current handoff. Run the tests with:

```sh
cargo test --workspace
```

## License

AGPL-3.0-or-later; see [LICENSE](LICENSE).
