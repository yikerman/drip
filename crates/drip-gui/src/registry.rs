//! The registries: node kinds the editor offers and loads, and node-specific
//! presentation, listed by hand; see the crate README.
use crate::model::Kind;
use crate::node_ui::{Binding, preview, scopes};
use drip::registry::Registry;
use std::sync::LazyLock;

/// Nodes that only present their inputs; what they prepare is GUI-only.
pub static VIEWS: &[&Kind] = &[
    &preview::observe::KIND,
    &scopes::histogram::observe::KIND,
    &scopes::waveform::observe::KIND,
    &scopes::vectorscope::observe::KIND,
];

/// Kinds the editor offers and projects may contain.
pub static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    Registry::new(&[
        drip::registry::NODES,
        VIEWS,
        #[cfg(test)]
        crate::test_nodes::NODES,
    ])
});

/// Node-specific presentation; every other kind uses the schema controls.
pub(crate) static BINDINGS: &[&[Binding]] = &[
    &[
        Binding::new::<drip::node::exposure::Definition>(),
        Binding::new::<drip::node::sigmoid::apply::Definition>(),
        Binding::new::<drip::node::perspective::Definition>(),
        Binding::new::<preview::observe::Definition>(),
        Binding::new::<scopes::histogram::observe::Definition>(),
        Binding::new::<scopes::waveform::observe::Definition>(),
        Binding::new::<scopes::vectorscope::observe::Definition>(),
    ],
    #[cfg(test)]
    crate::test_nodes::BINDINGS,
];

pub fn binding(kind: &Kind) -> Option<&'static Binding> {
    BINDINGS.iter().copied().flatten().find(|binding| std::ptr::eq(binding.kind(), kind))
}
