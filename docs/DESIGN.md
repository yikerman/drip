# Design

Decisions and rationale. Development rules live in [AGENTS](../AGENTS.md),
contracts beside the code, validation in [PROGRESS](PROGRESS.md), unfinished work
in [TODO](../TODO.md). Status: requirement, decided, tentative, or open.

## Direction

- **Decided:** The next milestone is a packaged proof of concept that users can
  try. The user accepts current image quality and editing workflows as sufficient
  for that scope, based on the established darktable algorithms and existing
  verification. Prioritize installation, launch and a complete RAW-to-TIFF smoke
  test. Missing modules such as lens correction do not block the proof of concept.
  Broader reference-based processing validation, workflow refinement and SpyderX
  measurements remain follow-up work; this scope decision does not establish
  physical color accuracy or validate untested platforms. CLI expansion and
  speculative optimization remain deferred.
- **Decided:** Frontend binaries use their package names: `drip-cli` and
  `drip-gui`. This keeps CLI Rustdoc output separate from the `drip` library.
- **Decided:** Keep `gpu-allocator` and the pinned `wgpu-hal` on the same
  `windows` crate version (currently 0.62.2); their DX12 interfaces cross crate
  boundaries. CI uses `--locked` to validate the committed dependency resolution.
- **Requirement:** Rust 1.95 is the minimum supported toolchain.
- **Requirement:** Correct display color on Wayland, macOS and Windows. Linux is
  the primary development platform; X11 is unsupported. A replacement GUI must
  cover all three color paths.
- **Requirement:** LibRaw handles decoding; LittleCMS handles ICC transforms.
- **Decided:** Use Rayon. The CubeCL trial lost on latency and development effort
  at host-valued node boundaries. Reconsider GPU execution with measured
  whole-node gains, including transfers and allocations. Sub-300 ms latency
  is acceptable without further optimization.

## Packaging

- **Requirement:** Distribute binaries with minimal dependencies for users to
  install. Static native processing libraries are part of the initial proof of
  concept; the normal Linux C/C++ runtime may remain a system dependency.
- **Decided:** `drip-raw` owns the safe decoding API, C shim and native RAW stack.
  Root `vendor/` contains unmodified Git submodules checked out at release tags;
  Gitlinks pin their exact commits. Initialize them during checkout/CI setup.
  Compilation does not fetch or update sources. Pins and licenses are recorded
  in [THIRD_PARTY](../THIRD_PARTY.md).
- **Decided:** Cargo builds LibRaw through `cc`, reading the pinned upstream
  translation-unit list from `Makefile.am` on every platform [18]; the MSVC
  makefile lists the same sources, while `cc` supplies Cargo's compiler/runtime
  settings and output paths. JPEG/zlib use their upstream CMake builds. Generated
  files live under `OUT_DIR`; build scripts track vendored inputs explicitly and
  select platform settings from Cargo's target environment. The shim uses the
  bundled headers and static-library declarations, including on Windows.
  Normalize checkout line endings before parsing the source manifest. MSVC-family
  compilers use upstream's `/EHsc` so decoder exceptions unwind local allocations.
- **Decided:** Keep reentrant LibRaw with JPEG/zlib decoding, without OpenMP,
  internal LittleCMS, RawSpeed, DNG SDK or sample programs. Preserve the complete
  upstream source list; only reference-processing shim functions are gated by
  `reference`. JPEG SIMD is required; x86 builds need NASM. Codecs compile in
  Release mode, while LibRaw/the shim follow Cargo's profile. Compiler runtimes
  follow the target toolchain rather than forcing static libstdc++ on Linux.
- **Decided:** Defer LibRaw OpenMP: acquiring and distributing its runtime across
  platforms adds packaging work beyond the initial proof of concept. Revisit
  decoder performance and upstream platform restrictions once runtime packaging
  is resolved; track the work in TODO.
- **Decided:** Enable `lcms2`'s bundled `static` feature once at workspace level
  for the library and GUI. Preserve explicit `LCMS2_LIB_DIR` overrides, which
  take precedence over that feature in the locked `lcms2-sys` build script.
- **Decided:** Cargo's `dist` profile owns build settings: release
  optimization, thin Rust LTO and stripped debug information, retaining unwinding.
  Rust and native libraries keep toolchain CPU defaults. Defer x86-64-v3 rather
  than maintaining architecture-specific compiler policy. Preserve
  Cargo target selection and user compiler flags. Stable Cargo is sufficient;
  no nightly requirement or fast-math is introduced.
- **Decided:** CI runs a three-platform test matrix on Linux, macOS and Windows.
  Linux is required; macOS and Windows failures are tolerated. All three finish
  before the distribution matrix starts, and a Linux failure blocks distribution.
  Keep fail-fast disabled so each platform reports its result.
- **Decided:** Build distribution binaries natively for AMD64 and ARM64 on each
  platform. GitHub provides [runners for all six combinations](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
  avoiding cross-compiler, SDK and native-library sysroot configuration.
  Distribution build failures remain failures; tolerated test failures do not
  validate artifacts. Disable distribution matrix fail-fast so a failed target
  does not cancel other builds or their uploads.
- **Decided:** CI uses the newest standard images listed in
  [actions/runner-images](https://github.com/actions/runner-images#available-images).
  Use `-latest` aliases when they select the newest image for the architecture;
  otherwise use the newest explicit label. Linux uses Ubuntu 26.04 on both
  architectures, replacing the earlier Ubuntu 22.04 baseline. Linux runtime
  requirements follow the build image; an AppImage wrapper cannot lower them [17].
  Minimum supported systems and runtime-loaded graphics, Wayland and file-dialog
  dependencies still need clean-desktop verification.
- **Tentative:** Package Linux `.tar.xz` archives and Flatpaks, macOS `.app`
  bundles in ZIP files, and Windows ZIP files, using the same Cargo builds.
- **Decided:** Each distribution job uploads the assembled `./dist/` directory
  as a ZIP under its platform/architecture artifact name; missing output is an
  error. `cargo xtask dist [--target <triple>]` builds `drip-gui` and `drip-cli`
  with the `dist` profile and stages only their executables. Clap parses the CLI;
  Cargo's artifact messages supply paths and platform suffixes. Preserve Cargo's
  default target when none is supplied and leave compiler environments untouched.
  Replace the staging directory after a successful build to exclude stale files.
- **Open:** Platform app bundles and runtime packaging beyond the CI ZIPs.
  GitHub's artifact ZIPs drop executable permissions; Linux/macOS users must
  restore them after extraction. The distributed CLI remains a scaffold.
  Source archives must include submodule contents. Record compiler/source versions,
  build settings and notices with artifacts.
- **Open:** macOS deployment target, Windows runtime packaging and native platform
  verification. Linux baseline CI and clean-desktop smoke tests must pass before
  claiming compatibility; local compilation alone does not establish it.

References [16]–[18] are in [THIRD_PARTY](../THIRD_PARTY.md).

## DAG

- **Decided:** Concrete port types replace capability reflection and output
  inference. Working RGB uses linear Rec.2020/D65 throughout creative processing.
  Fixed output contracts remove downstream type propagation; local checks cover
  relationships between actual inputs.
- **Decided:** Export is an explicit action. Capture metadata arrives through a
  separate optional edge: provenance and pixel interpretation have different
  lifetimes.
- **Decided:** Keep strict project JSON and opaque frontend state; prototype
  formats have no migration guarantee. Template inputs are ordinary parameters.

### Future operations

These constraints guide future payloads; production implementations remain in TODO.

- **Requirement:** Sensor calibration needs matching sensor sites, CFA phase and
  sample units. Separate decoding from black subtraction/normalization/crop, or
  retain enough information to account for them. Spatial gains may require
  spatial saturation/noise metadata.
- **Requirement:** Chart calibration produces a reusable transform with a stated
  basis, normalization, reference illumination and fit residuals. Chart and
  subject can have different grids and exposures.
- **Requirement:** Masks, gain fields and scalar samples have distinct contracts
  despite shared storage. Range, broadcasting, division and geometry rules belong
  to the consuming operation. Index-wise blending may pair unrelated scenes;
  sensor calibration requires sensor-site correspondence. A mask applied after
  filtering differs from a mask restricting the filter's samples.
- **Requirement:** Capture provenance and model applicability stay separate.
  Processing must update or discard metadata whose assumptions it changes.
- **Decided:** Multiple-frame operations consume decoded-value collections.
  Alignment may be internal or a reusable output; no graph-level frame-stack
  abstraction is needed.

## Processing and interaction

- **Decided:** Sensor processing precedes preview reduction to retain clipping
  information. Reduction belongs to demosaic integration. One manual detail level
  applies across the graph; navigation leaves it unchanged and export uses full
  detail.
- **Decided:** Evaluate every GUI consumer, including off-screen ones. Keep old
  views while work runs and replace them on current failure. The worker owns
  evaluation, preparation and large-image retirement to keep UI latency bounded.
- **Decided:** Exports retain the graph/resource snapshot captured at dispatch,
  including across project replacement. Preview/export share decoded resources;
  normalized pyramids and full-detail export graph caches are not retained.
- **Decided:** Canvas is the main workspace. Pop-outs are WM-arranged OS windows,
  with no docking or persisted pop-out layout. Canvas text scales with the layer,
  accepting softness at large zoom. Idle windows should rest.
- **Decided:** Cross-camera validation uses CC0 raw.pixls.us fixtures and the
  original Sony photograph. Git/LFS owns integrity; unusable metadata is tracked
  separately from unsupported decoders. Export parity alone cannot establish
  colorimetric accuracy.

## Logging

- **Decided:** Frontends configure stderr `log`, defaulting to
  `warn,drip=info,drip_gui=info`. Kernels return errors; the frontend owns reporting.

| Level | Events |
|-------|--------|
| Error | Failed explicit open/save/action, stopped worker, unusable display/window. |
| Warn | New live-preview failure, display fallback/recovery, unavailable parenting. |
| Info | Startup/version, GPU/output selection, project open/save, action start/completion. |
| Debug | Evaluation/timing, resources, invalidation, windows, incomplete configuration and refused edits. |
| Trace | Cache hits, discarded requests, frame timings and repaint scheduling. |

- **Decided:** Report each changed root preview failure once after accepting its
  generation; reset clears history. Action failures remain errors, including
  incomplete configuration. Retain dependency origin and relevant node, detail,
  generation, window or destination context. Display selection owns the fallback
  warning and its cause.
