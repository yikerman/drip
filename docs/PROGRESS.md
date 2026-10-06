# Progress handoff

Current outcomes and verification limits. Decisions are in [DESIGN](DESIGN.md),
unimplemented work in [TODO](../TODO.md); older experiments remain in Git history.

## 2026-10-05: dropdown wheel navigation

- Shared dropdowns across parameters, Preview detail and pop-out zoom. Wheel
  navigation follows menu order and stops at either end. Reused slider scroll
  accumulation so small trackpad deltas work without scrolling the parent panel.

- Custom preview zooms appear between presets, allowing scrolling to either
  neighboring preset. Existing parameter reset menus remain available.

- Validation: 47 GUI tests passed, covering bounded dropdown navigation,
  trackpad accumulation, parent scrolling, parameter reset, project detail
  updates and custom zoom selection. Clippy with warnings denied, formatting
  and diff checks passed. The GPU-only test remains ignored; live gestures
  were not checked.

## 2026-10-05: preview pop-out navigation

- Added Fit and percentage presets, pointer-centered wheel zoom and left-drag
  panning to image pop-outs. Pixel zoom follows each window's desktop scale;
  rendered dimensions and reduction clarify the active preview detail.

- Navigation reuses the color-managed texture and stays local to the pop-out.
  Image updates retain the viewed location; zoom does not trigger processing.

- Validation: 45 GUI tests passed, including desktop-scale geometry, pointer
  anchoring, pan bounds and pop-out gestures. Clippy with warnings denied,
  formatting and diff checks passed. The GPU-only test remains ignored;
  live window interaction and moving between monitors were not checked.

## 2026-10-05: logging levels and failure context

- Separated UI status from logging. Default output covers session/file actions,
  display setup and failures; evaluation/resource diagnostics use debug, with
  cache/frame/scheduling detail at trace under module targets.

- Added typed incomplete configuration for RAW/profile/output paths. Accepted
  previews report changed root failures once, including node and generation;
  actions retain dependency causes and log destination, outcome and duration.
  Display fallback emits one warning with its protocol/capability cause.

- Validation: 134 workspace tests and three doctests passed; clippy with warnings
  denied, formatting and diff checks passed. The GPU-only test remains ignored;
  live display fallback and native window failure paths were not exercised.

## 2026-10-05: gamut-check sampling and parallel ICC conversion

- Feed Lab into LittleCMS's native gamut checker to improve sampling near black.
  Preview and export share parallel ICC conversion with the mutable pixel cache
  disabled. Softproof retains its bounded device-RGB round trip.

- On the Sony fixture at half detail (3984 × 2660, LittleCMS 2.16, 12 workers),
  release gamutcheck fell from 2.8–3.0 s to 0.68–0.69 s; softproof fell from
  3.5–3.6 s to 0.51–0.57 s. These exclude upstream processing and GUI presentation.
  Cyan coverage fell from 24.7% to 3.9%. The native mask remains approximate near
  the boundary; it does not test exact RGB coordinate bounds.

- Validation: 137 workspace tests and three doctests passed. Preview tests also
  passed with bundled LittleCMS 2.19, covering dark colors, round-trip agreement
  away from the sampled boundary and identical output across worker counts.
  Clippy with warnings denied, formatting and diff checks passed. The GPU-only
  test remains ignored; live GUI behavior was not checked.

## 2026-10-05: documentation spacing

- Added visible line breaks to node help and paragraph spacing to project docs.
  Existing wording is unchanged; only the new proofing docs were shortened.

- Validation: wording comparison, 14 GUI app tests, formatting and diff checks
  passed. Live appearance is untested.

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
