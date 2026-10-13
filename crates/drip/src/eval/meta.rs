//! A meta node's sub-graph, spliced into the run while the meta node runs.
//!
//! Its nodes are keyed from what they read, demanded from the meta node's
//! outputs, executed in sub-graph order and released after their last
//! consumer, through the same slots as the graph's nodes. They report their
//! own timings and failures. A sub-graph holds no meta node, so at most one
//! expansion is active at a time.
use super::run::{Run, Slot, descriptions};
use crate::{
    Result,
    expand::{Source, SubGraph},
    graph::Entry,
    image::Image,
    ports::NodeId,
};
use std::sync::Arc;

/// The sub-graph of the meta node running now.
pub(super) struct Expansion {
    pub graph: Arc<SubGraph>,
    /// The slots the meta node's inputs read, in its input order.
    feeds: Vec<Option<Slot>>,
}
impl Expansion {
    pub fn holds(&self, id: NodeId) -> bool {
        id.graph == self.graph.dag.id
    }

    /// The values sub-graph node `entry` reads at `scale`: outputs of other
    /// sub-graph nodes as published, and the meta node's inputs as the meta
    /// node read them.
    pub fn inputs(&self, entry: &Entry, scale: u32) -> Vec<Option<Slot>> {
        (0..entry.inputs.len())
            .map(|port| match self.graph.source(entry, port)? {
                Source::Node(from) => Some(Slot::output(from, scale)),
                Source::Meta(index) => self.feeds[index],
            })
            .collect()
    }
}

impl Run<'_, '_> {
    /// Run meta node `e` at `scale` through its sub-graph, expanded from
    /// `inputs`, which it read from `feeds`. Returns the sub-graph's outputs
    /// in the meta node's port order, which the meta node's key keeps.
    pub(super) fn expand(
        &mut self,
        e: &Entry,
        scale: u32,
        feeds: &[Option<Slot>],
        inputs: &[Option<Image>],
    ) -> Result<Vec<Image>> {
        let key = self.keys[&(e.id, scale)].clone()?;
        let graph = self.store.expansion(key, || e.node.expand(&descriptions(inputs)))?;
        self.expansion = Some(Expansion { graph: graph.clone(), feeds: feeds.to_vec() });
        let outputs = self.run_expansion(&graph, scale);
        // The sub-graph's nodes belong to this execution. Another meta node
        // with an equal key expands to the same sub-graph and runs it again.
        self.running.retain(|(id, _)| id.graph != graph.dag.id);
        self.keys.retain(|(id, _), _| id.graph != graph.dag.id);
        self.expansion = None;
        outputs
    }

    fn run_expansion(&mut self, graph: &SubGraph, scale: u32) -> Result<Vec<Image>> {
        for entry in graph.entries() {
            let key = self.key(entry.id, scale);
            self.keys.insert((entry.id, scale), key);
        }
        let outputs: Vec<_> = graph.outputs().map(|port| Slot::output(port, scale)).collect();
        for &slot in &outputs {
            self.demand(slot);
        }
        let failures = self.execute_all(self.order(&graph.dag));
        let mut images = Vec::with_capacity(outputs.len());
        for slot in outputs {
            images.push(match failures.get(&(slot.port.node, scale)) {
                Some(error) => {
                    self.release(slot);
                    Err(error.clone())
                }
                None => self.take(slot),
            });
        }
        images.into_iter().collect()
    }
}
