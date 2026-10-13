//! The memo: materialized outputs kept across evaluations, keyed by content.
//!
//! An edit usually changes a few nodes, so most outputs of the next demand
//! equal outputs the evaluator already computed. Each output's [`Key`] hashes
//! what determines it: the producer's kind and version, its [`Identity`] and
//! the keys of its inputs. Evaluation runs a node only when a demanded output
//! is missing. The store keeps rasters within the runtime's [`Budget`] per
//! side and evicts the least durable, least recently used representations
//! first.
use crate::{
    definition::{Durability, FileStamp, Identity, Kind},
    image::{Image, Side},
    runtime::{Budget, GlobalContext},
};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::UNIX_EPOCH};

/// A SHA-256 content key. Outputs with equal keys hold equal samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key([u8; 32]);

impl Key {
    /// The key of a node's outputs: `kind` and its version, `identity`, the
    /// global scale for a computed identity, and the key of each input in
    /// port order, `None` where the input is unconnected.
    pub fn node(
        kind: &Kind,
        identity: &Identity,
        global: &GlobalContext,
        inputs: &[Option<Key>],
    ) -> Self {
        let mut hash = Hasher::new(b"drip node");
        hash.bytes(kind.id.as_bytes());
        hash.u64(kind.version.into());
        match identity {
            Identity::Computed(parameters) => {
                hash.tag(0);
                hash.json(parameters);
                hash.u64(global.scale.into());
            }
            Identity::Loaded(parameters, files) => {
                hash.tag(1);
                hash.json(parameters);
                hash.u64(files.len() as u64);
                files.iter().for_each(|file| hash.file(file));
            }
            Identity::Bound(instance) => {
                hash.tag(2);
                hash.0.update(instance.to_le_bytes());
            }
        }
        hash.u64(inputs.len() as u64);
        for input in inputs {
            match input {
                None => hash.tag(0),
                Some(key) => {
                    hash.tag(1);
                    hash.0.update(key.0);
                }
            }
        }
        hash.finish()
    }

    /// The key of output `index` of the node keyed `self`.
    pub fn output(self, index: usize) -> Self {
        let mut hash = Hasher::new(b"drip output");
        hash.0.update(self.0);
        hash.u64(index as u64);
        hash.finish()
    }
}

/// Writes every field self-delimiting, by length or type tag, so distinct
/// field sequences hash distinct byte strings.
struct Hasher(Sha256);
impl Hasher {
    fn new(domain: &[u8]) -> Self {
        let mut hash = Self(Sha256::new());
        hash.bytes(domain);
        hash
    }
    fn finish(self) -> Key {
        Key(self.0.finalize().into())
    }
    fn tag(&mut self, tag: u8) {
        self.0.update([tag]);
    }
    fn u64(&mut self, value: u64) {
        self.0.update(value.to_le_bytes());
    }
    fn bytes(&mut self, bytes: &[u8]) {
        self.u64(bytes.len() as u64);
        self.0.update(bytes);
    }
    /// Objects hash their fields in key order, so the encoding is canonical
    /// whatever order the JSON map keeps.
    fn json(&mut self, value: &Json) {
        match value {
            Json::Null => self.tag(0),
            Json::Bool(value) => self.tag(if *value { 2 } else { 1 }),
            Json::Number(number) => {
                self.tag(3);
                self.bytes(number.to_string().as_bytes());
            }
            Json::String(text) => {
                self.tag(4);
                self.bytes(text.as_bytes());
            }
            Json::Array(values) => {
                self.tag(5);
                self.u64(values.len() as u64);
                values.iter().for_each(|value| self.json(value));
            }
            Json::Object(fields) => {
                self.tag(6);
                self.u64(fields.len() as u64);
                let mut fields: Vec<_> = fields.iter().collect();
                fields.sort_unstable_by_key(|(name, _)| *name);
                for (name, value) in fields {
                    self.bytes(name.as_bytes());
                    self.json(value);
                }
            }
        }
    }
    fn file(&mut self, file: &FileStamp) {
        self.bytes(file.path.as_os_str().as_encoded_bytes());
        self.u64(file.len);
        // Times before the epoch take their own tag, apart from times after it.
        let modified = file.modified.map(|time| match time.duration_since(UNIX_EPOCH) {
            Ok(after) => (1, after),
            Err(before) => (2, before.duration()),
        });
        match modified {
            None => self.tag(0),
            Some((tag, offset)) => {
                self.tag(tag);
                self.u64(offset.as_secs());
                self.u64(offset.subsec_nanos().into());
            }
        }
    }
}

/// Materialized outputs by key, within a byte budget per side.
pub(crate) struct Store {
    entries: HashMap<Key, Entry>,
    budget: Budget,
    /// Advances on every insertion and hit, and orders entries by last use.
    clock: u64,
    evictions: usize,
}
struct Entry {
    image: Image,
    durability: Durability,
    used: u64,
}

impl Store {
    pub fn new(budget: Budget) -> Self {
        Self { entries: HashMap::new(), budget, clock: 0, evictions: 0 }
    }

    /// The image stored under `key`, marked as used now.
    pub fn get(&mut self, key: Key) -> Option<Image> {
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.used = self.clock;
        Some(entry.image.clone())
    }

    /// Keep `image`, which must be materialized, under `key`, then trim. An
    /// entry already under `key` stays and is marked as used: its samples are
    /// equal, and entries are never replaced.
    pub fn insert(&mut self, key: Key, image: Image, durability: Durability) {
        debug_assert!(image.footprint().is_some(), "the store keeps materialized images");
        self.clock += 1;
        let used = self.clock;
        self.entries.entry(key).or_insert(Entry { image, durability, used }).used = used;
        self.trim();
    }

    /// Drop representations until each side is within the budget: device
    /// first, then host, each time from the entry with the lowest durability
    /// and, among those, the oldest use. An entry that loses its last
    /// representation leaves the store. Representations change after
    /// insertion, when a consumer uploads or downloads an entry, so evaluation
    /// trims again when it ends.
    pub fn trim(&mut self) {
        for (side, limit) in [(Side::Device, self.budget.device), (Side::Host, self.budget.host)] {
            let bytes = |entry: &Entry| {
                entry.image.footprint().expect("stored images are materialized").on(side)
            };
            let mut total: u64 = self.entries.values().map(bytes).sum();
            while total > limit {
                let (&key, entry) = self
                    .entries
                    .iter_mut()
                    .filter(|(_, entry)| bytes(entry) > 0)
                    .min_by_key(|(_, entry)| (entry.durability, entry.used))
                    .expect("a side over budget has an entry holding bytes");
                total -= bytes(entry);
                self.evictions += 1;
                match entry.image.without(side) {
                    Some(rest) => entry.image = rest,
                    None => drop(self.entries.remove(&key)),
                }
            }
        }
    }

    /// Representations dropped since the store was made.
    pub fn evictions(&self) -> usize {
        self.evictions
    }
}
