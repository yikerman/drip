# Drip GUI

Ownership follows the [library's organization](../drip/README.md#organization):
node-specific controls, preparation and presentation live under `src/node_ui/`.

## Files

The non-trivial files outside `node_ui/`:

- `app.rs`: application state: the project, windows, layout, evaluation
  requests and node actions.
- `app/commands.rs`: project commands shared by the menu and shortcuts, the
  unsaved-edits prompt, opening and saving, undo/redo.
- `history.rs`: undo history of whole-graph snapshots.
- `editing.rs`: applies a frame's graph edits and reports their effects.
- `editor.rs`, `editor/picker.rs`: the canvas (node frames, ports, wires, pan
  and zoom) and the node picker.
- `model/`: `Graph`, the DAG plus names, template inputs and layout, and its
  project persistence.
- `registry.rs`: the hand-maintained `VIEWS`, `REGISTRY` and `BINDINGS`.
- `templates.rs`: built-in templates, the repository's `templates/*.drip-template`
  files embedded in the binary and listed by hand.
- `worker.rs`: the background queue for previews, details and node actions.
  A preview that waits for a foreign job, such as RAW decode, keeps its
  views, names the node in the status line and runs again when the job ends.
  A detail is a view's input over the region its window shows at 100 % or
  more, evaluated at full detail after the preview and drawn over it.
- `parent.rs`: pop-outs as child windows of the main window.
- `render/`: worker-prepared node drawing, linear Rec.2020 images and
  color-managed presentation per window system.

## Registries

`src/registry.rs` lists, by hand, everything the GUI discovers:

- `VIEWS`: node kinds declared in the GUI because what they prepare is GUI-only
  (preview and scopes). They use the library's `view` and `scope` tags.
- `REGISTRY`: the kinds the editor offers and projects may contain,
  `drip::registry::NODES` followed by `VIEWS`.
- `BINDINGS`: node-specific presentation, an `impl GuiNode` on a node's
  generated `Definition` (custom controls, a prepared view or a frontend
  action). Kinds without an entry use schema controls and their declared
  action.

Test builds append the lists in `src/test_nodes.rs`, which hold test-only kinds
and bindings that project loading or prepared views must find.

## Presenting a node

A node listed in `drip::registry::NODES` needs no GUI code: the canvas, its
ports, the inspector's help and the schema controls come from its
declaration. Add GUI code only to override that. For the library's
[`double` example](../drip/README.md#adding-a-node), a note under its
controls goes in `src/node_ui/double.rs`:

```rust
use super::{ControlCx, GuiNode};
use drip::node::double;
use egui::Ui;

impl GuiNode for double::compute::Definition {
    type Presentation = ();
    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        node.schema(ui);
        ui.small("Equivalent to Exposure at +1 EV.");
    }
}
```

1. Declare the module: `mod double;` in `src/node_ui/mod.rs`.
2. List `Binding::new::<drip::node::double::compute::Definition>()` in
   `registry::BINDINGS`. Without this the override is ignored and the schema
   controls remain.

Other overrides use the same impl and entry:

- `PREPARE` with a `Presentation` draws a view in the node and its pop-out,
  from the node's typed inputs, as in `scopes/histogram.rs`.
- `ACTION` adds a frontend action whose result may become new parameters, as
  in `exposure.rs` (auto exposure). A kind has a frontend action or one declared
  on the node, not both.
- A view whose window edits the node implements `Drawable::interact` and sets
  parameters through `ControlCx::set`, as in `perspective.rs` (Perspective
  Transform's handles).

A node that only presents its inputs and prepares GUI-only data, such as a
scope, is declared here with `#[drip::node]` instead of in the library, listed
in `registry::VIEWS` as well as `BINDINGS`, and tagged `view`.
