//! The expansion of a meta node: its sub-graph's nodes keyed from the meta
//! node's inputs and run in its place.
use super::run::{Run, Slot, descriptions, output_id};
use crate::{Result, expand::Source, graph::Entry, image::Image, ports::NodeId, store::Key};
use std::collections::{HashMap, HashSet};

impl Run<'_, '_> {
    /// Expand meta node `e`, running at `scale` on `inputs` read from
    /// `slots`, then run the nodes of its sub-graph whose demanded outputs
    /// the store does not hold, at `scale`. Returns the sub-graph's outputs
    /// in the meta node's port order. The sub-graph's nodes are keyed from
    /// the keys of the meta node's inputs, as if they were in the graph.
    pub(super) fn meta(
        &mut self,
        e: &Entry,
        scale: u32,
        slots: &[Option<Slot>],
        inputs: &[Option<Image>],
    ) -> Result<Vec<Image>> {
        let key = self.keys[&(e.id.index, scale)].clone()?;
        let graph = self.store.expansion(key, || e.node.expand(&descriptions(inputs)))?;
        let outer: Vec<_> = slots.iter().map(|slot| slot.and_then(|s| self.slot_key(s))).collect();
        let mut keys: HashMap<NodeId, Key> = HashMap::new();
        for entry in graph.entries() {
            let inputs: Vec<_> = (0..entry.inputs.len())
                .map(|port| match graph.source(entry, port)? {
                    Source::Node(from) => Some(keys[&from.node].output(from.index)),
                    Source::Meta(index) => outer[index],
                })
                .collect();
            let key = Key::node(entry.node.kind(), &entry.node.identity()?, scale, &inputs);
            keys.insert(entry.id, key);
        }
        // Walk back from the outputs as `demand` does: a stored value is a
        // hit, and a missing one runs its node.
        let mut values = HashMap::new();
        let mut running = HashSet::new();
        let mut demanded: Vec<_> = graph.outputs().collect();
        while let Some(port) = demanded.pop() {
            if values.contains_key(&port) || running.contains(&port.node) {
                continue;
            }
            if let Some(image) = self.store.get(keys[&port.node].output(port.index)) {
                self.hits += 1;
                values.insert(port, image);
                continue;
            }
            running.insert(port.node);
            let entry = graph.dag.entry(port.node)?;
            demanded.extend((0..entry.inputs.len()).filter_map(|port| {
                match graph.source(entry, port)? {
                    Source::Node(from) => Some(from),
                    Source::Meta(_) => None,
                }
            }));
        }
        for entry in graph.entries().filter(|entry| running.contains(&entry.id)) {
            let reads: Vec<_> = (0..entry.inputs.len())
                .map(|port| match graph.source(entry, port)? {
                    Source::Node(from) => Some(values[&from].clone()),
                    Source::Meta(index) => inputs[index].clone(),
                })
                .collect();
            let images = self.run(entry.node.as_ref(), scale, &reads)?;
            self.nodes += 1;
            for (port, image) in images.into_iter().enumerate() {
                values.insert(output_id(entry.id, port), image);
            }
        }
        let outputs: Vec<_> = graph.outputs().collect();
        // The run stores the meta node's outputs under its own key.
        for (port, image) in &values {
            if running.contains(&port.node)
                && !outputs.contains(port)
                && image.footprint().is_some()
            {
                let durability = graph.dag.entry(port.node)?.node.kind().durability();
                self.store.insert(keys[&port.node].output(port.index), image.clone(), durability);
            }
        }
        Ok(outputs.iter().map(|port| values[port].clone()).collect())
    }
}
