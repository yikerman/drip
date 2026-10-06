# Progress handoff

Current outcomes and verification limits. Decisions are in [DESIGN](DESIGN.md),
unimplemented work in [TODO](../TODO.md); older experiments remain in Git history.

## 2026-10-05: preview proofing

- Added normal, softproof and cyan gamut-warning modes to Preview, sharing
  profile settings and conversion with Export. The display renderer is unchanged.
- Older saved Preview nodes need the new parameters under the strict file policy.
- Validation: 130 workspace tests, three doctests, clippy and formatting passed;
  one GPU test remains ignored. Proofing tests passed with LittleCMS 2.16 and
  2.19, including agreement with bounded TIFF export. Live appearance is untested.

## 2026-10-05: categorized node selection

- Split node-kind metadata into stable type ID, internal category and default
  display name. Built-in names are capitalized; graph instances keep their
  editable names through the existing Rename/inspector path. Project files keep
  their type IDs and serialized labels, including custom names.
- Extracted the Add node selector into a small module that groups and sorts
  categories and names, returning the chosen kind to the editor. Menu styling
  now uses one inherited style function for both parent menus and submenus.
- Updated README and tracked the future dedicated picker in TODO.
- Validation: 124 workspace tests and three compile-fail doctests passed; the
  GPU-only test remained ignored. Checks include menu order/selection, renaming
  without changing type metadata, and serialized custom-name round trips.
  Workspace clippy with warnings denied, formatting and diff checks passed.
  Appearance has not been checked in the running GUI.

## 2026-10-05: parameter control gestures

- Added wheel adjustment to the shared parameter sliders, consuming panel scroll
  while hovered. Fractional wheel input accumulates in egui memory; graph values
  and schema defaults remain authoritative.
- Right-click opens Reset to default on sliders, checkboxes, dropdowns and path
  filenames, and in the existing parameter-name menu. Removed double-click reset
  after the user reported conflicts with single-click behavior. Inspector,
  pop-outs and template inputs use the same renderer. README usage documents
  these gestures.
- Slider setters reject right-button edits before egui paints, preventing the
  temporary value jump when opening Reset. A regression test reproduced the
  old mismatch and now checks displayed values on press, drag and release.
- Validation: 41 GUI tests passed, including reset menus, right-click preserving
  displayed values, wheel bounds, trackpad accumulation and panel scroll isolation. The
  GPU-only test remained ignored. Clippy with warnings denied, formatting and
  diff checks passed. The final gestures have not been checked live.

## 2026-10-05: port navigation and connection gestures

- Unified input/output port behavior: left-click opens connected-node navigation,
  left-drag pans, and right-click starts/completes a connection. Invalid targets
  preserve existing wiring; Escape/background right-click cancels the pending wire.
- Right-click disconnects the nearest hovered wire through a constant screen-space
  hit band. Nodes and controls retain input priority; wires thicken and node
  bodies gain a thin border on hover.
- Split README setup into Build & install and Usage, documenting the gesture
  rationale, navigation, connection, cancellation and disconnection behavior.
- Validation: all 36 GUI tests passed, including port navigation, connection
  cancellation/rejection, wire hit bands at three zoom levels, crossings and
  node/port input priority. The GPU-only test remained ignored. Clippy with
  warnings denied, formatting and diff checks passed. Live GUI appearance and
  gestures have not been checked.

## 2026-10-05: canvas and node gestures

- Implemented the agreed interaction table: left-drag pans over background and
  nodes, right-drag moves nodes, and right-click opens node Rename / Delete.
  Rename selects the node and focuses its inspector label field on the next pass.
- Headless interaction tests cover selection, panning/movement at three zoom
  levels, drag/menu separation, and rename/delete/add actions. All 26 GUI tests
  passed; the GPU-only test remained ignored. Clippy with warnings denied,
  formatting and diff checks passed. Live GUI gestures have not been checked.

## 2026-10-05: context menu contrast

- Added a lighter neutral background and a thin dark border to the canvas and
  parameter right-click menus through a shared theme helper.
- Validation: formatting and `cargo check -p drip-gui --offline` passed.
  Appearance has not been checked in the running GUI.

## 2026-10-05: semantic source cleanup

- Grouped demosaicing with bin2x2 and preview reduction, and scopes with shared
  settings. Grouped GUI rendering/shaders and moved reusable parameter panels
  out of the inspector. Public node exports and serialized node IDs are retained.
- Compacted project notes, linking detailed code contracts instead of repeating
  them. Kept cohesive kernels, application orchestration and worker code intact.
- Validation: 106 workspace tests, three compile-fail doctests and the explicit
  GPU color-output test passed. Clippy with warnings denied, formatting, rustdoc
  and local document links passed. The user also exercised the release GUI on
  Intel/LNL: passthrough tagging, evaluations and clean exit were confirmed.

## 2026-10-05: current presentation validation

- Shared extended-linear output and explicit Wayland color tagging implemented;
  the pipeline is documented in [display](../crates/drip-gui/src/render/display.rs).
- Prior GPU readback passed for wide-gamut images, GUI colors, scene clipping,
  out-of-volume blending and both sRGB fallback formats against independent
  colorimetry/LittleCMS references. GUI tests, clippy, formatting and rustdoc passed.
- Intel/KWin traces verified BT.709/linear, Rec.2020 target and relative intent
  over three window lifecycles. Missing extended-target support and injected
  setup failures each selected sRGB and exited normally over three runs.
- Current output remains unverified on NVIDIA, macOS and other compositors.
  Windows is best effort and untested, including HDR desktop-white handling.
  Screenshots do not establish physical accuracy; instrument verification remains.

## 2026-10-05: processing and editing baseline

- Typed graph contracts, optional metadata input, sigmoid, RCD, inpaint opposed,
  scopes and shared node controls are implemented. Detailed behavior, source
  deviations and reference vectors live beside the implementations/tests.
- Earlier workspace validation covered synthetic/reference algorithms, real RAW
  export, graph/cache lifetimes, compile-fail contracts and headless GUI behavior.
  Exiftool/exiv2 readback matched the ARW capture metadata; capture time was checked
  under UTC, Tokyo and Los Angeles. Broader photo/camera validation remains.
- Canvas/pop-out interaction was exercised by the user and Wayland parenting
  tested live. Hidden-window pacing and native dialogs were not retested after
  their fixes. macOS/Windows setup and packaging remain unverified locally.

## Performance evidence

Historical timings explain choices; they are not current latency guarantees.
On the Sony fixture, Ryzen 5600G/12 workers and RTX 3080, the 2026-10-04 trial
measured Rayon at 162 ms versus CubeCL CPU 582 ms/GPU 623 ms, with a retained RAW
pyramid. Removing that pyramid reduced decoded-resource storage from about
226 MB to 85 MB, excluding node results and temporaries.

The 2026-10-05 RCD/highlight pipeline measured release level-change medians of
1.20 s at 1/2 and 2.64 s at Full over seven cycles with 12 workers. These include
sensor processing/cache replacement, exclude GUI preparation/upload/draw, and
have uncontrolled filesystem caches. Exposure/sigmoid edits reuse upstream
results. Reproduce with [preview_latency](../crates/drip/examples/preview_latency.rs)
and measure whole frames before tuning.
