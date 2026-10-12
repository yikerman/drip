# Drip GUI

Ownership follows the [library's organization](../drip/README.md#organization):
node-specific controls, preparation and presentation live under `src/node_ui/`.

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
