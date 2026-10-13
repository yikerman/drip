# Drip GUI

The interactive frontend: a node canvas, pop-out windows and colour-managed
previews over the `drip` library. Node-specific controls, preparation and
presentation live under `src/node_ui/`, one module per node, as the library
keeps node code under `node/`.

## Build and run

```sh
cargo run --release -p drip-gui -- [project.drip]
```

The default build computes through wgpu. To use a native runtime, build it
in and select it with `DRIP_BACKEND` (see the
[drip README](../drip/README.md#build-and-test)):

```sh
cargo build --release -p drip-gui --features drip/vulkan
DRIP_BACKEND=vulkan target/release/drip-gui
```

On macOS use `--features drip/metal,drip/metal-native`. On Windows the
default build uses D3D12, which PowerShell can name with
`$env:DRIP_BACKEND='dx12'`. The backend does not change the GUI renderer.

Logs go to stderr. `RUST_LOG=warn,drip=debug,drip_gui=debug` shows
processing diagnostics, `drip_gui::frame=trace` frame timings and
`drip_gui::worker=trace` per-node host timings. `DRIP_HISTORY` sets the undo
depth, 100 by default.

## Files

| File | Owns |
|---|---|
| `main.rs` | OS windows and the event loop |
| `app.rs`, `app/commands.rs` | the project, windows, layout and evaluation requests, and the commands the menu and shortcuts share |
| `editing.rs` | a frame's graph edits and their effects |
| `history.rs` | undo history of whole-graph snapshots |
| `editor.rs`, `editor/picker.rs` | the canvas and the node picker |
| `inspector.rs` | the selected node's help and controls |
| `model/` | `Graph`: the DAG plus names, template inputs and layout, and its persistence |
| `worker.rs` | the background queue for previews, full-detail crops and actions |
| `registry.rs` | the hand-kept `VIEWS`, `REGISTRY` and `BINDINGS` |
| `templates.rs` | built-in templates, embedded from `templates/` |
| `node_ui/` | node-specific controls, preparation and views |
| `render/` | drawing prepared views and colour-managed presentation per window system |
| `parent.rs` | pop-outs as child windows of the main window |

`registry.rs` lists everything the GUI discovers. `REGISTRY` is
`drip::registry::NODES` followed by `VIEWS`, the kinds declared here because
what they prepare is GUI-only: Preview and the three scopes. `BINDINGS`
lists node-specific presentation. Test builds append the kinds and bindings
in `test_nodes.rs`.

## Presenting a node

A node listed in `drip::registry::NODES` needs no GUI code: the canvas,
its ports, the inspector's help and the schema controls come from its
declaration. Add GUI code only to override that. Auto exposure, in
`src/node_ui/exposure.rs`, adds a frontend action to Exposure:

```rust
use super::{Action, GuiNode};
use drip::node::{data::Rec2020, exposure};

impl GuiNode for exposure::Definition {
    type Presentation = ();
    const ACTION: Option<Action<Self>> = Some(Action {
        name: "Auto exposure",
        run: |_, inputs, ctx| {
            let image = inputs.image.require::<Rec2020>()?;
            Ok(Some(exposure::meter(&image, &ctx.kernels())?))
        },
    });
}
```

1. Declare the module in `src/node_ui/mod.rs`.
2. List `Binding::new::<exposure::Definition>()` in `registry::BINDINGS`.
   Without it the binding is ignored.

The action's result becomes the node's new parameters, applied as an
ordinary edit. The same impl can also override:

- `controls`, to replace the schema controls, as in `sigmoid.rs`.
- `PREPARE`, to draw a view in the node and its pop-out:
  `Prepare::Inputs` from the node's typed inputs, as in `preview/mod.rs`, or
  `Prepare::Outputs` from a reduction's typed outputs, as in
  `scopes/histogram.rs`.
- `Drawable::interact`, for a view whose window edits the node through
  `ControlCx::set`, as in `perspective.rs`.

A kind has a frontend action or one declared on the node, never both.
A view node that prepares GUI-only data is declared here with
`#[drip::node]`, tagged `view`, and listed in both `VIEWS` and `BINDINGS`.
The scopes are bins nodes tagged `scope` too: their `#[cube]` bin bodies
are drip-gui's only device code, and preparation reaches the device only
through drip.
