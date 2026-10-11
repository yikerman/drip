//! Transactional graph edits checked on ports alone: payloads, occupancy and
//! cycles. Contracts depend on loaded data, so only evaluation runs them.
use crate::{Error, Result, definition::Node, payload::Payload, ports::*};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone)]
pub(crate) struct Entry {
    pub id: NodeId,
    pub node: Arc<dyn Node>,
    pub inputs: Vec<Option<PortId>>,
}
#[derive(Clone)]
pub struct Dag {
    pub(crate) id: u64,
    pub(crate) entries: Vec<Option<Entry>>,
    pub(crate) order: Vec<usize>,
}
impl Default for Dag {
    fn default() -> Self {
        Self::new()
    }
}
impl Dag {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self { id: next(&NEXT), entries: Vec::new(), order: Vec::new() }
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &dyn Node)> {
        self.entries.iter().flatten().map(|e| (e.id, e.node.as_ref()))
    }
    pub fn node(&self, id: NodeId) -> Result<&dyn Node> {
        Ok(self.entry(id)?.node.as_ref())
    }
    pub fn edges(&self) -> impl Iterator<Item = (PortId, PortId)> + '_ {
        self.entries.iter().flatten().flat_map(|e| {
            e.inputs.iter().enumerate().filter_map(move |(index, from)| {
                from.map(|from| (from, PortId { node: e.id, index, direction: Direction::Input }))
            })
        })
    }
    pub fn input_port(&self, node: NodeId, name: &str) -> Result<PortId> {
        self.port(node, name, Direction::Input)
    }
    pub fn output_port(&self, node: NodeId, name: &str) -> Result<PortId> {
        self.port(node, name, Direction::Output)
    }
    fn port(&self, node: NodeId, name: &str, direction: Direction) -> Result<PortId> {
        let kind = self.entry(node)?.node.kind();
        let ports = match direction {
            Direction::Input => kind.inputs,
            Direction::Output => kind.outputs,
        };
        let index = ports.iter().position(|p| p.name == name).ok_or_else(wrong_handle)?;
        Ok(PortId { node, index, direction })
    }
    pub fn set_parameters(&mut self, id: NodeId, parameters: serde_json::Value) -> Result<()> {
        let kind = self.entry(id)?.node.kind();
        self.replace(id, (kind.build)(parameters)?)
    }
    /// Edits either pass together or leave the graph intact.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Edit<'_>) -> Result<T>) -> Result<T> {
        let mut draft =
            Self { id: self.id, entries: self.entries.clone(), order: self.order.clone() };
        let result = f(&mut Edit { dag: &mut draft })?;
        draft.order = draft.topological_order()?;
        *self = draft;
        Ok(result)
    }
    pub fn add(&mut self, node: Arc<dyn Node>) -> Result<NodeId> {
        self.edit(|e| Ok(e.add(node)))
    }
    pub fn connect<P: Payload>(&mut self, from: Output<P>, to: Input<P>) -> Result<()> {
        self.connect_ids(from.id, to.id)
    }
    pub fn connect_ids(&mut self, from: PortId, to: PortId) -> Result<()> {
        self.edit(|e| e.connect_ids(from, to))
    }
    pub fn disconnect<P>(&mut self, to: Input<P>) -> Result<()> {
        self.edit(|e| e.disconnect(to.id))
    }
    pub fn replace(&mut self, id: NodeId, node: Arc<dyn Node>) -> Result<()> {
        self.edit(|e| e.replace(id, node))
    }
    pub fn remove(&mut self, id: NodeId) -> Result<()> {
        self.edit(|e| e.remove(id))
    }
    pub fn typed_output<P: Payload>(&self, port: PortId) -> Result<Output<P>> {
        self.output_of::<P>(port)?;
        Ok(Output::new(port.node, port.index))
    }
    pub(crate) fn entry(&self, id: NodeId) -> Result<&Entry> {
        if id.graph != self.id {
            return Err(wrong_handle());
        }
        self.entries
            .get(id.index)
            .and_then(Option::as_ref)
            .filter(|e| e.id == id)
            .ok_or_else(wrong_handle)
    }
    pub(crate) fn output(&self, id: PortId) -> Result<&Entry> {
        let e = self.entry(id.node)?;
        if id.direction != Direction::Output || id.index >= e.node.kind().outputs.len() {
            return Err(wrong_handle());
        }
        Ok(e)
    }
    /// Typed handles are checked too: generated `Ports` accept any node.
    pub(crate) fn output_of<P: Payload>(&self, id: PortId) -> Result<&Entry> {
        let e = self.output(id)?;
        if !e.node.kind().outputs[id.index].is::<P>() {
            return Err(wrong_handle());
        }
        Ok(e)
    }
    fn input(&self, id: PortId) -> Result<&Entry> {
        let e = self.entry(id.node)?;
        if id.direction != Direction::Input || id.index >= e.inputs.len() {
            return Err(wrong_handle());
        }
        Ok(e)
    }
    fn topological_order(&self) -> Result<Vec<usize>> {
        let mut degrees = vec![0usize; self.entries.len()];
        let mut children = vec![Vec::new(); self.entries.len()];
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(e) = entry else { continue };
            for (port, from) in e.inputs.iter().enumerate() {
                let Some(from) = from else { continue };
                let source = self.output(*from)?;
                if source.node.kind().outputs[from.index].payload_type
                    != e.node.kind().inputs[port].payload_type
                {
                    return Err(Error::Contract(format!(
                        "payload mismatch at {}.{}",
                        e.node.kind().id,
                        e.node.kind().inputs[port].name
                    )));
                }
                degrees[index] += 1;
                children[from.node.index].push(index);
            }
        }
        let mut ready: VecDeque<_> = degrees
            .iter()
            .enumerate()
            .filter(|(i, n)| **n == 0 && self.entries[*i].is_some())
            .map(|(i, _)| i)
            .collect();
        let mut order = Vec::new();
        while let Some(index) = ready.pop_front() {
            order.push(index);
            for &child in &children[index] {
                degrees[child] -= 1;
                if degrees[child] == 0 {
                    ready.push_back(child);
                }
            }
        }
        if order.len() != self.entries.iter().flatten().count() {
            return Err(Error::Graph("connection creates a cycle".into()));
        }
        Ok(order)
    }
}

pub struct Edit<'a> {
    dag: &'a mut Dag,
}
impl Edit<'_> {
    pub fn add(&mut self, node: Arc<dyn Node>) -> NodeId {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id =
            NodeId { graph: self.dag.id, index: self.dag.entries.len(), generation: next(&NEXT) };
        self.dag.entries.push(Some(Entry {
            id,
            inputs: vec![None; node.kind().inputs.len()],
            node,
        }));
        id
    }
    pub fn connect<P: Payload>(&mut self, from: Output<P>, to: Input<P>) -> Result<()> {
        self.connect_ids(from.id, to.id)
    }
    pub fn connect_ids(&mut self, from: PortId, to: PortId) -> Result<()> {
        self.dag.output(from)?;
        if self.dag.input(to)?.inputs[to.index].is_some() {
            return Err(Error::Graph("input is already connected".into()));
        }
        self.dag.entries[to.node.index].as_mut().unwrap().inputs[to.index] = Some(from);
        Ok(())
    }
    pub fn disconnect(&mut self, to: PortId) -> Result<()> {
        self.dag.input(to)?;
        self.dag.entries[to.node.index].as_mut().unwrap().inputs[to.index] = None;
        Ok(())
    }
    pub fn replace(&mut self, id: NodeId, node: Arc<dyn Node>) -> Result<()> {
        // One static kind per node type, so equal kinds have equal ports.
        if !std::ptr::eq(self.dag.entry(id)?.node.kind(), node.kind()) {
            return Err(Error::Graph("replacement must preserve node kind and port schema".into()));
        }
        self.dag.entries[id.index].as_mut().unwrap().node = node;
        Ok(())
    }
    pub fn remove(&mut self, id: NodeId) -> Result<()> {
        self.dag.entry(id)?;
        self.dag.entries[id.index] = None;
        for e in self.dag.entries.iter_mut().flatten() {
            for from in &mut e.inputs {
                if from.is_some_and(|p| p.node == id) {
                    *from = None;
                }
            }
        }
        Ok(())
    }
}

/// A process-unique identity. A u64 counter does not wrap in practice.
fn next(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed)
}
