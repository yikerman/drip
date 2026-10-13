//! Values produced during one evaluation, keyed by output port.
//!
//! Evaluation runs a demanded subgraph once and hands each output to its
//! consumers. The store holds an output from the moment its node publishes it
//! until its last consumer is released, so peak memory follows the live
//! edges. Memoization across evaluations with content keys, durability, the
//! host and disk tiers and a device byte budget will extend this table.
use crate::{image::Image, ports::PortId};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Store {
    values: HashMap<PortId, Image>,
    uses: HashMap<PortId, usize>,
}
impl Store {
    /// Register one more consumer of `port`. Only retained ports are kept.
    pub fn retain(&mut self, port: PortId) {
        *self.uses.entry(port).or_default() += 1;
    }

    /// Keep `image` for the consumers of `port`, or drop it if there are none.
    pub fn insert(&mut self, port: PortId, image: Image) {
        if self.uses.contains_key(&port) {
            self.values.insert(port, image);
        }
    }

    pub fn get(&self, port: PortId) -> Option<&Image> {
        self.values.get(&port)
    }

    /// One consumer of `port` is done; the last one drops the value.
    /// `port` must be retained.
    pub fn release(&mut self, port: PortId) {
        let remaining = self.uses.get_mut(&port).expect("released port is retained");
        *remaining -= 1;
        if *remaining == 0 {
            self.values.remove(&port);
        }
    }
}
