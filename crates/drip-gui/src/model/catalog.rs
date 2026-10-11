//! Discovery of every registered node kind; labels, controls and ports come
//! from the kinds' own static declarations.
use drip::definition::{Kind, NODES};
use std::{collections::BTreeMap, sync::LazyLock};

static KINDS: LazyLock<BTreeMap<&'static str, &'static Kind>> = LazyLock::new(|| {
    let mut kinds = BTreeMap::new();
    for registration in NODES {
        let kind = registration.kind;
        assert!(kinds.insert(kind.id, kind).is_none(), "duplicate node ID {}", kind.id);
    }
    kinds
});
pub fn get(id: &str) -> Option<&'static Kind> {
    KINDS.get(id).copied()
}
#[derive(Default)]
pub struct Registry;
impl Registry {
    pub fn kinds(&self) -> impl Iterator<Item = &'static Kind> {
        KINDS.values().copied()
    }
    pub fn get(&self, id: &str) -> Option<&'static Kind> {
        get(id)
    }
}
