//! Transactional connection checks over small CPU descriptions, never pixels.
use crate::{Error, Result, definition::Node, payload::Payload, ports::*};
use std::{
    any::TypeId,
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
    pub descs: Vec<Option<Description>>,
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
        #[allow(deprecated)] // Keep compatibility with Rust 1.95.
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .expect("graph identity exhausted");
        Self { id, entries: Vec::new(), order: Vec::new() }
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
        let index = self
            .entry(node)?
            .node
            .inputs()
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(wrong_handle)?;
        Ok(PortId { node, index, direction: Direction::Input })
    }
    pub fn output_port(&self, node: NodeId, name: &str) -> Result<PortId> {
        let index = self
            .entry(node)?
            .node
            .outputs()
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(wrong_handle)?;
        Ok(PortId { node, index, direction: Direction::Output })
    }
    pub fn set_parameters(&mut self, id: NodeId, parameters: serde_json::Value) -> Result<()> {
        let kind = self.entry(id)?.node.metadata().id;
        self.replace(id, crate::definition::registered(kind, parameters)?)
    }
    /// All descriptor-affecting edits either pass together or leave the graph intact.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Edit<'_>) -> Result<T>) -> Result<T> {
        let mut draft =
            Self { id: self.id, entries: self.entries.clone(), order: self.order.clone() };
        let result = f(&mut Edit { dag: &mut draft })?;
        draft.validate()?;
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
        let e = self.output(port)?;
        if !e.node.outputs()[port.index].is::<P>() {
            return Err(wrong_handle());
        }
        Ok(Output::new(port.node, port.index))
    }
    /// Full-detail description checked during graph edits. Evaluation reports
    /// its request-specific description in the returned value.
    pub fn description<P: Payload>(&self, output: Output<P>) -> Result<Option<&P::Desc>> {
        let e = self.output(output.id)?;
        if e.node.outputs()[output.id.index].payload != TypeId::of::<P>() {
            return Err(wrong_handle());
        }
        Ok(e.descs[output.id.index].as_ref().map(downcast))
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
        if id.direction != Direction::Output || id.index >= e.node.outputs().len() {
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
    fn validate(&mut self) -> Result<()> {
        let order = self.topological_order()?;
        for &index in &order {
            self.check_contract(index)?;
        }
        self.order = order;
        Ok(())
    }
    fn topological_order(&self) -> Result<Vec<usize>> {
        let mut degrees = vec![0usize; self.entries.len()];
        let mut children = vec![Vec::new(); self.entries.len()];
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(e) = entry else { continue };
            for (port, from) in e.inputs.iter().enumerate() {
                let Some(from) = from else { continue };
                let source = self.output(*from)?;
                if source.node.outputs()[from.index].payload != e.node.inputs()[port].payload {
                    return Err(Error::Contract(format!(
                        "payload mismatch at {}.{}",
                        e.node.metadata().id,
                        e.node.inputs()[port].name
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
    fn check_contract(&mut self, index: usize) -> Result<()> {
        let e = self.entries[index].as_ref().unwrap();
        // Templates can be wired before their source assets are supplied.
        // Every edit rechecks known relationships at full detail. Evaluation
        // repeats contracts for its global context before running each node.
        let descs = e
            .inputs
            .iter()
            .map(|from| match from {
                None => Ok(None),
                Some(from) => Ok(self.output(*from)?.descs[from.index].clone()),
            })
            .collect::<Result<Vec<_>>>()?;
        let outputs = e.node.contract(&Default::default(), &descs)?;
        if outputs.len() != e.node.outputs().len() {
            return Err(Error::Graph("node contract output arity mismatch".into()));
        }
        self.entries[index].as_mut().unwrap().descs = outputs;
        Ok(())
    }
}

pub struct Edit<'a> {
    dag: &'a mut Dag,
}
impl Edit<'_> {
    pub fn add(&mut self, node: Arc<dyn Node>) -> NodeId {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated)] // Keep compatibility with Rust 1.95.
        let generation = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .expect("node identity exhausted");
        let id = NodeId { graph: self.dag.id, index: self.dag.entries.len(), generation };
        self.dag.entries.push(Some(Entry {
            id,
            inputs: vec![None; node.inputs().len()],
            descs: vec![None; node.outputs().len()],
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
        let old = self.dag.entry(id)?;
        let same_ports = |a: Vec<PortSpec>, b: Vec<PortSpec>| {
            a.len() == b.len()
                && a.iter().zip(&b).all(|(a, b)| {
                    a.name == b.name
                        && a.payload == b.payload
                        && a.placement == b.placement
                        && a.optional == b.optional
                })
        };
        if old.node.metadata().id != node.metadata().id
            || !same_ports(old.node.inputs(), node.inputs())
            || !same_ports(old.node.outputs(), node.outputs())
        {
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
