# Drip design notes

Every decision has a status:

- **requirement**: the user's goal. Changing it needs the user's agreement.
- **decided**: agreed with the user. On 2026-10-04 the user approved the session-1 design as a whole, with the answers recorded per entry.
- **tentative**: proposed by the engineer and not yet agreed.
- **open**: no proposal yet, or a proposal is waiting for the user.

Each entry gives its rationale. Superseded entries are struck through and stay in place so the history remains readable.

## 1. Structure and platforms

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| S1 | Cargo workspace with three crates: `drip` (library), `drip-cli` (binary `drip`), `drip-gui` (binary `drip-gui`). | requirement | All processing, the graph, color management and persistence go in the library. Frontends stay thin, and the library never depends on a GUI. |
| S2 | Linux on Wayland is primary. Windows and macOS must build in CI but are tested only loosely. X11 is unsupported. | requirement | |
| S3 | License: AGPL-3.0-or-later. | decided | The user chose AGPL. It is compatible with LibRaw (LGPL-2.1 or CDDL-1.0) and LittleCMS (MIT). Still to confirm: `-or-later` or `-only`. |
| S4 | CI runs fmt, clippy and tests on Linux, and builds on Windows and macOS. | decided | From the handoff. Native dependencies (LibRaw, LittleCMS) will need per-OS install steps once they're added (see L3). |

## 2. Libraries

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| L1 | Raw decoding uses LibRaw only. Color management uses LittleCMS through the `lcms2` crate only. | requirement | |
| L2 | Write our own minimal LibRaw binding: a small C shim compiled with `cc` against the LibRaw headers, plus one safe Rust function `decode(path) -> Result<RawData>` that copies everything we need into owned Rust values and closes the LibRaw handle before it returns. | decided | Survey of existing bindings, October 2026: **rsraw 0.1.1** is unsound, because `RawImage::open(&[u8])` hands the buffer to `libraw_open_buffer`, which keeps the pointer, but `RawImage` has no lifetime tied to the buffer (use-after-free). It also leaks the handle when open fails, has no MSVC support, needs bindgen/libclang at build time, and vendors LibRaw 0.21.3. **libraw-rs-sys** vendors LibRaw 0.20.1 and was last touched in 2021. **libraw/libraw-sys** date from 2015. Putting a C shim in front of LibRaw means Rust never mirrors LibRaw's struct layout, which changes between releases, and keeps the unsafe surface to a handful of functions. No LibRaw handle ever escapes, so thread safety only requires linking the reentrant `libraw_r`. |
| L3 | Linking: system `libraw_r` through pkg-config on Linux (Fedora ships 0.22.2 and needs `LibRaw-devel`), Homebrew on macOS, vcpkg on Windows. Vendoring the LibRaw source is the fallback if those prove unreliable in CI. | decided | The system library gets security fixes from the distribution. Vendoring makes builds reproducible, but then we own LibRaw's build. |
| L4 | TIFF is written with the pure-Rust `tiff` crate, and the ICC profile goes in tag 34675. | decided | The requirements don't name a TIFF library. Still to check: the encoder must write arbitrary tags and 16-bit and float samples. |
| L5 | `lcms2` transforms are created per export and confined to that call. No native handle (LibRaw or LittleCMS) ever lives inside a `Value`. | decided | Values are `Send + Sync` by construction. Thread safety of `lcms2` `Transform` depends on its context type, and is checked against the pinned crate version when M2 starts. |

## 3. Processing model

### 3.1 Values and port types

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| P1 | Port types are **semantic** and drawn from a closed runtime enum, `PortType`. Values form a matching enum, `Value`, whose payloads are immutable and held in an `Arc`, so values are cheap to clone and `Send + Sync`. | decided | Connections are type-checked at runtime: the GUI checks a connection as it's made, and the loader checks a whole file. That needs types as runtime values that can be compared and serialized. A closed enum keeps matches exhaustive and serialization trivial, and adding a type is a local change. An open `TypeId`/`dyn Any` scheme would allow plugins but loses both of those properties, and nothing calls for plugins. |
| P2 | Prototype types: `Mosaic` (1-channel f32 CFA data with its CFA pattern), `CameraRgb`, `SceneRec2020` (scene-referred linear Rec.2020), `DisplayRec2020` (display-referred linear Rec.2020, nominally 0 to 1), `RawMetadata`. The three RGB types are 3-channel interleaved f32. Every image carries its geometry relative to the sensor (scale and origin), so later ROI work and spatial parameters have a coordinate system. | decided | Keeping scene-referred and display-referred Rec.2020 as separate types makes "tone map before the output transform" a rule the type system enforces: Export accepts only `DisplayRec2020`. |
| P3 | **Colorimetric characterization travels with the image.** `Mosaic` and `CameraRgb` carry an `Arc<CameraInfo>`: the XYZ→camera matrix (LibRaw's confusingly named `cam_xyz`) and the as-shot multipliers. WB and camera→Rec.2020 read it from their image input. The raw reader still has a `RawMetadata` output (exposure, lens, timestamps) for informational nodes and future EXIF passthrough. | decided | If the matrix and multipliers travelled on a separate metadata wire, a user could wire one file's metadata into another file's image and get silently wrong colors, which type checking can't catch. This refines the handoff idea rather than contradicting it, since the reader still outputs both. |
| P6 | White-balance state is **not** tracked in the types for now. A graph that applies WB twice still type-checks. | decided | The user's call (2026-10-04). It keeps types simple and allows debayer-then-WB pipelines. |
| P4 | An input port accepts a *set* of types. An output port has exactly one type. A connection is valid when the output's type is in the input's set. An input takes at most one edge. An output may feed any number of inputs. | decided | This covers Histogram accepting any image type, with no subtyping machinery. Preview accepts only `SceneRec2020` and `DisplayRec2020`, since it has no display interpretation for a mosaic or camera RGB. Viewers attach as branches. Inserting one inline would need a pass-through output, which they deliberately don't have. |
| P5 | Bayer only in the prototype. The raw reader rejects X-Trans, Foveon and linear DNG with an error. The CFA pattern is represented generally (w×h color indices) so X-Trans can be added later. | decided | 2×2 binning has no meaning for X-Trans. The check sits at the input boundary, which is the only place checks belong. |

### 3.2 Nodes

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| N1 | Separate the **node kind** from the **node instance**. A kind is a static descriptor in a registry: stable name (e.g. `raw.read`), version, parameter schema, static input and output ports, `eval`, `migrate`, actions. An instance is plain data: `{ id, kind, params, ui }`. | decided | Instances are then trivially serializable and clonable, and an instance of an unknown kind is still a valid instance (see F3). Behavior lives in one place per kind. Ports are static. Variadic nodes would need ports computed from params, which nothing needs yet. |
| N2 | Parameters are declared by a schema (`name`, kind: float with range / int / bool / enum / path / ICC profile / …, default, flags) and stored as a map from name to `ParamValue`. `eval` receives them through a typed accessor. | decided | One declaration serves the GUI (widgets), persistence (defaults and tolerance), the CLI (`--set node.param=value`) and templates (the per-image flag, see F4). |
| N3 | `eval(params, inputs: &[Value], ctx: &EvalContext) -> Result<Evaluated>`, where `Evaluated = { outputs: Vec<Value>, view: Option<View> }` and `View` is its own enum (`Histogram`, `Image`), separate from port values. Eval must be deterministic and free of side effects. | decided | Determinism is what makes the caching in E2 sound. `view` is explained in U1. |

### 3.3 Evaluation

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| E1 | **Pull-based.** A frontend asks for a set of target nodes. The evaluator topologically sorts their ancestors and evaluates only those. | decided | The CLI asks only for what it exports, and the GUI asks only for what is visible. Unreachable or broken branches cost nothing, and UI-only nodes never get in the CLI's way, with no special case anywhere. |
| E2 | Change detection by **dependency stamps**. A node's stamp hashes its kind, version, canonical params, the evaluation scale and the revisions of the external resources it reads (raw files, ICC files), together with, for each connected input, the input name, the source output name and the source's stamp. A node is recomputed only when its stamp differs from its cached entry. One entry is cached per node. | decided | "Recompute when it or something upstream changed" falls straight out of this, with no dirty flags to keep in sync. Files aren't detected changing on disk. A manual reload bumps the resource's revision, which invalidates the decoded resource, all downstream results and any cached errors. Resource revisions arrive with the first resource-reading node in M2. The stamp also covers the version the registry resolves and the identity of each source node, so that swapping registries, or rewiring between identical nodes, never serves a stale result or an error that names the wrong node. |
| E3 | Errors are values, kept per node. A failing node records its error, its descendants are marked blocked, and nodes in other branches still evaluate. | decided | The GUI can show the error on the node itself while the rest of the graph keeps working. |
| E4 | `EvalContext` carries a downscale level `k`, which gives `scale() = 2^k`. Storing the level makes non-power-of-two scales unrepresentable. Later it will also carry a region of interest and a cancellation token. | decided | This leaves room for the deferred items: 1:1 viewing of the visible region (ROI), background evaluation (cancellation) and GPU work. None of them is built now. |
| E5 | **Preview scale is applied in the raw reader**: after black subtraction, same-color CFA sites are averaged with the CFA phase preserved, and odd edges are cropped, so the reduced image is still a valid Bayer mosaic. Spatial parameters are expressed in full-resolution pixels and divided by `ctx.scale` inside nodes. | decided | Interactive editing must work on a reduced image, but debayering needs a real mosaic. Binning inside the CFA satisfies both. Expressing parameters at full resolution keeps preview and export consistent. |
| E8 | Dev builds use `opt-level = 1`. | decided (user, 2026-10-04) | At opt-level 0 the real-raw tests took about 105 s, against 11 s at opt-level 1. Image processing has to be usable in development. |
| E6 | The interactive session keeps one cache at preview scale. Full-resolution export has no persistent cache: within one run each node is evaluated once, and its outputs are held until their last consumer has run. | decided | A 45 MP 3-channel f32 image takes about 540 MB, so caching every node at full resolution isn't viable. Cache budgets and eviction (decoded raws included) still need a policy. |
| E7 | LibRaw's decode (scale-independent and expensive) is memoized in a session resource cache keyed by path. It is not node state. | decided | Changing preview scale shouldn't re-decode the raw file. Keeping this out of the nodes keeps nodes stateless. |

### 3.4 UI-only nodes and side effects

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| U1 | UI-only nodes are ordinary nodes with zero output ports. What they compute for presentation (histogram bins, the preview image) goes into `Evaluated::view`, which any node may set. The library computes views. Frontends read and draw them. | decided | Nothing is special-cased. A histogram in the middle of a pipeline is just another target the GUI asks for. Views are plain data, so they can be tested headlessly. |
| U2 | Side effects are **actions**. A node kind may declare named actions, each with its own evaluation policy; Export declares `export` with policy full scale. A frontend calls `run_action(node, "export")`. The library then evaluates that node's inputs against a snapshot of the graph and resources and runs the action with those inputs and params. `eval` itself never writes files. The CLI runs `export` on every node that declares it. Still to define: what happens on overwrite, on destination collisions between exports, and when some exports fail. | decided | This meets "side effects on explicit action, not on every re-evaluation" and gives export its full-resolution evaluation. The alternative, treating export as a sink that writes during eval, fails three ways: the cache would skip a repeated export, the preview scale would leak into the export, and a stray GUI request could write files. |

## 4. Color

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| C1 | Prototype pipeline: raw.read → white_balance → debayer.bin2x2 → camera_to_rec2020 → tonemap.sigmoid → export.tiff, with preview and histogram as UI-only nodes. | requirement | |
| C2 | Normalization in raw.read works on the untouched `raw_image` after `unpack`. LibRaw's `subtract_black` is not called. Per site, black = `black + cblack[c] + cblack[6..]` pattern at the site's (row, col) in sensor coordinates. Every site is divided by one common denominator, `maximum − black`, as dcraw does. Values below black are kept negative, not clamped. Then crop to the visible area. Verified against LibRaw 0.22 source: `unpack()` doesn't call `adjust_bl()`, so the raw fields hold. The denominator's common black is what `adjust_bl` derives. Patterns of at most 2×2 are first folded into the per-color offsets, so it becomes the smallest black of a Bayer cell. Otherwise it is `black + min(cblack[0..4]) + min(pattern)`. The second green keeps its own as-shot multiplier when the camera records one. The black pattern is indexed by visible-area coordinates. | decided | LibRaw's black level model has several layers. Its `adjust_bl` folds the common black into `cblack`, so mixing processing stages subtracts black twice. A per-channel denominator would act as a hidden white balance. Keeping values below black avoids biasing the noise floor. |
| C3 | Camera→Rec.2020 follows dcraw [1]. With `A` = XYZ→camera (`cam_xyz`) and `B` = Rec.2020→XYZ (D65), `T = inverse(diag(1 / (A·B·1)) · A · B)`. The as-shot multipliers, normalized to green = 1, are applied first, by the WB node. Prototype is 3-color Bayer only. If the multipliers or the matrix are missing or invalid, raw.read fails. | decided | Going straight to Rec.2020 uses one matrix and doesn't depend on LibRaw's sRGB-targeted `rgb_cam`. Row normalization maps white-balanced camera neutral to D65 white. That is still an adaptation model (von Kries in camera space), so "no chromatic adaptation" means "no separate CAT step such as Bradford". The user confirmed this reading. |
| C4 | Tone mapping: per channel, `y = x^c / (x^c + k)` on exposed scene values `x = max(0, v · 2^exposure)` (Michaelis–Menten/Naka–Rushton form), with `k = g^c (1/g − 1)` for g = 0.18 so middle grey maps to itself. Parameters: exposure (EV, default 0), contrast c (default 1.5). It maps [0, ∞) onto [0, 1). | decided (user, 2026-10-04) | It's the simplest curve with a fixed grey point, a contrast control and a soft shoulder. Per-channel application desaturates and shifts hue in bright saturated areas. The user asked to keep it this simple for the prototype, with no color adaptation (no hue preservation or gamut mapping). A better tone mapper is in TODO. |
| C5 | Export goes `DisplayRec2020` → LittleCMS transform → user ICC profile → TIFF, with the profile embedded. The source profile is built in memory as Rec.2020 primaries, D65 and a linear TRC. Accepted output profiles: RGB color space, class display, output or color space, checked when the user picks the file. Parameters: rendering intent, black-point compensation, sample format (`u16` or `f32`), compression (none, or deflate with a level). The `tiff` crate offers deflate levels fast (1), balanced (6) and best (9). u16 output is clipped to [0, 1] explicitly before encoding. | decided (user, 2026-10-04) | Taken literally, "arbitrary ICC profile" covers gray, CMYK, device-link and abstract profiles, which aren't interchangeable RGB destinations. Whether LittleCMS float transforms stay unbounded depends on the profile: tabulated TRCs and LUT profiles clamp. Perceptual intent does nothing for matrix/shaper profiles. A float TIFF with an arbitrary ICC profile is unusual, and readers handle it inconsistently. |
| C6 | 2×2 binning debayer halves each dimension. A "full-resolution" export therefore evaluates the full sensor data but writes a half-size TIFF. | requirement consequence | Stated explicitly so it isn't mistaken for a bug. |

### 4.1 Display path

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| D1 | The library produces linear Rec.2020 views. Turning them into display output is the frontend's job, using library helpers (LittleCMS) where an app-side transform is needed. | decided | This keeps the library independent of any GUI. |
| D2 | **The compositor does the display transform.** The GUI presents an scRGB (`ExtendedSrgbLinear`, `Rgba16Float`) swapchain, which the Vulkan WSI tags for the compositor. Rec.2020 content is converted into it with one 3×3 matrix and scaled so that SDR reference white is 203/80 = 2.5375. Drip has no app-side display transform for now. | decided (user, 2026-10-04) | Spike on the dev machine (KWin 6.7.5, NVIDIA 615): the WSI sends `set_primaries_named(srgb)`, `set_tf_named(ext_linear)`, `set_luminances(0, 80, 203)`. The user asked about simply declaring the surface as Rec.2020. That works on KWin, but only if Drip tags the `wl_surface` itself through a second Wayland connection on winit's display and relies on the WSI leaving sRGB swapchains untagged. That behavior is undocumented, and wgpu deliberately drops `BT2020_LINEAR`/`PASS_THROUGH`. scRGB carries Rec.2020 exactly (negative values) and is also the native path on Windows and macOS. Either way, egui's sRGB UI and the images need converting into one space, so declaring Rec.2020 saves nothing. |
| D3 | App-side display transform (compositors without color management): deferred, see TODO. | decided | It follows from D2. |
| D4 | GUI toolkit: winit + wgpu + egui (`egui-winit` and `egui-wgpu`, not eframe). | decided (user, 2026-10-04) | See section 6. |

## 5. Persistence

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| F1 | Format: JSON through serde (`serde_json`). It is pretty-printed, which costs nothing extra in code and diffs better. | decided (user, 2026-10-04) | It is universal, scriptable (`jq`) and can be edited by hand. |
| F2 | File layout: `{ format: "drip", version, nodes: [{id, label, kind, kind_version, params, bindings, ui}], edges: [{from: [id, port], to: [id, port]}], arguments, ui }`. Node IDs are stable `u64`s that are never reused while a graph is open. `nodes` is a list, not a map keyed by ID, so a duplicate ID is an error instead of being silently collapsed by the JSON parser. Labels are unique and non-empty. Ports are referenced by name. `ui` fields are opaque JSON owned by the frontend (node positions, view state). Relative paths will resolve against the project file's directory (M2). | decided | These are the handoff's ideas. Giving the frontend an opaque `ui` blob lets the GUI persist its layout in the library's file format without the library knowing anything about the GUI. |
| F3 | Tolerance: each kind migrates its own parameters (renames follow bindings too) **and port names** from older `kind_version`s, so edges are rewritten as well. Missing params take defaults. Unknown params are kept and written back. Instances of unknown kinds, or of a `kind_version` newer than this build knows, load as **opaque nodes**: their params and edges are preserved, and they and their descendants are blocked from evaluation (E3). A malformed file (bad JSON, dangling node IDs) is an error. Unresolved nodes are not. | decided | A file opened by an older Drip doesn't lose data, so files survive the app's evolution in both directions. |
| ~~F4~~ | ~~Per-image params flagged in the schema, bound by node label.~~ | superseded by F5 | |
| F5 | **A template is a function.** Any node parameter can be *bound* to a named graph input instead of holding a literal (`node.bindings: {param → input}`). The graph's inputs are exactly the names used in bindings. A **template** is a graph. A **project** is a graph plus `arguments: {input → value}`. Effective params = the literal params with bound ones replaced by arguments. A missing argument fails the node it binds (E3). The CLI supplies arguments per raw file. | decided (user, 2026-10-04) | One mechanism covers templates, batch processing and multi-reader graphs, and nodes need no knowledge of it. An argument's validity is checked against the kind of every parameter it's bound to. |

## 6. GUI toolkit (open)

Evidence from the dev machine (Fedora 44, KWin 6.7.5, NVIDIA 615.71, GTK 4.22.5):

- **KWin**: `wp_color_manager_v1` v2, parametric only. It has named primaries including bt2020, named transfer functions `gamma22`, `ext_linear`, `bt1886` and `st2084_pq`, and the `windows_scrgb` feature.
- **NVIDIA Vulkan WSI on Wayland** offers `R16G16B16A16_SFLOAT` swapchains with `BT2020_LINEAR_EXT` and `EXTENDED_SRGB_LINEAR_EXT` color spaces, so an app that owns its surface can present linear wide-gamut content directly.
- **GTK 4.22**: compositor color management is opt-in (`GDK_DEBUG=color-mgmt`). In a spike that showed a Rec.2020-linear `GdkMemoryTexture`, GTK bound KWin's `wp_color_manager_v1` and destroyed it straight away, even with the flag set, and never tagged the surface. Not yet diagnosed (probably a feature or version mismatch). GTK owns the `wl_surface`, so Drip can't tag it itself.
- **wgpu 30** exposes `SurfaceColorSpace::ExtendedSrgbLinear` (scRGB), which can carry Rec.2020 through out-of-range values. It has no Rec.2020-linear variant.

Decided: **winit + wgpu + egui** (`egui-winit` and `egui-wgpu` directly, not eframe, so Drip configures the surface). egui renders into an ordinary sRGB offscreen texture. A final Drip pass composites the decoded UI and the Rec.2020 image views in linear light and writes an scRGB float swapchain tagged for the compositor. egui's renderer needs no patches. Node-editor widgets exist for egui (e.g. egui-snarl). It ports easily to Windows and macOS.

Caveat: blending *inside* egui stays in gamma space. "Composites everything in linear Rec.2020" holds for the UI and image layers, not for egui's own translucent widgets.

Details from Codex's review that the plan must cover: convert Rec.2020 into linear-sRGB coordinates while keeping negative and above-one values. Choose a reference-white scale, since Windows-scRGB defines 1.0 as 80 cd/m² and SDR white is commonly 203 cd/m². egui's offscreen target is a gamma-encoded UNORM texture with premultiplied alpha, so it has to be unpremultiplied, decoded and premultiplied again when composited. The driver's WSI owns surface tagging when it presents in scRGB, so Drip must not attach its own color-management surface as well.

Spike done (protocol level, see D2). Still to do in M3: verify what is actually displayed, with saturated and neutral patches, alpha edges and a numeric check of the presented values. egui renders into an offscreen gamma-encoded `Rgba16Float` target, where image views are drawn sign-preserving and extended-sRGB-encoded, so z-order stays egui's. A final pass decodes into the scRGB swapchain.

## 7. Milestones (proposed)

1. **M1: graph engine and persistence.** Types, kinds and registry, graph editing with type and cycle checks, pull evaluator with stamps and cache, views and actions, the JSON format with migration and opaque nodes. Tested headlessly with toy node kinds.
2. **M2: prototype pipeline.** LibRaw shim and binding, the six processing nodes plus histogram and preview views, LittleCMS export. Ends with a harness that turns a raw file into a TIFF. Tests: synthetic mosaics with known matrices; real raw fixtures (CC0 samples) checked against normalization and matrix values computed independently; TIFF round-trips that check the embedded ICC profile and the values.
3. **M3: GUI.** Display spike, then the toolkit decision, then node editor, parameter panels, preview and histogram, export action. KWin surface tagging is part of this milestone, because the chosen GUI's surface depends on it.
4. **M4: display color path completed.** The app-side fallback, monitor changes, measured verification.
5. Later: CLI implementation, then the deferred items in the handoff.

## References

[1] D. Coffin, "dcraw.c," `convert_to_rgb()` and `cam_xyz_coeff()`. [Online]. Available: https://www.dechifro.org/dcraw/
