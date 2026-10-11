//! Transactional connection checks over small CPU descriptions, never pixels.
use crate::{
    Error, Result,
    definition::{Node, Outcome},
    payload::Payload,
    ports::*,
};
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
    pub outcome: Outcome,
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
        let index = self
            .entry(node)?
            .node
            .kind()
            .inputs
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(wrong_handle)?;
        Ok(PortId { node, index, direction: Direction::Input })
    }
    pub fn output_port(&self, node: NodeId, name: &str) -> Result<PortId> {
        let index = self
            .entry(node)?
            .node
            .kind()
            .outputs
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(wrong_handle)?;
        Ok(PortId { node, index, direction: Direction::Output })
    }
    pub fn set_parameters(&mut self, id: NodeId, parameters: serde_json::Value) -> Result<()> {
        let kind = self.entry(id)?.node.kind().id;
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
        self.output_of::<P>(port)?;
        Ok(Output::new(port.node, port.index))
    }
    /// Full-detail description checked during graph edits. Evaluation reports
    /// its request-specific description in the returned value.
    pub fn description<P: Payload>(&self, output: Output<P>) -> Result<Option<&P::Desc>> {
        Ok(match &self.output_of::<P>(output.id)?.outcome {
            Outcome::Known(descs) => Some(downcast(&descs[output.id.index])),
            Outcome::Unknown(_) => None,
        })
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
    fn check_contract(&mut self, index: usize) -> Result<()> {
        let e = self.entries[index].as_ref().unwrap();
        // Templates can be wired before their sources are bound or complete.
        // Every edit rechecks known relationships at full detail; evaluation
        // repeats contracts for its global context before running each node.
        let outcome = match self.known_inputs(e) {
            Ok(descs) => e.node.contract(&Default::default(), &descs)?,
            Err(reason) => Outcome::Unknown(reason),
        };
        if let Outcome::Known(descs) = &outcome
            && descs.len() != e.node.kind().outputs.len()
        {
            return Err(Error::Graph("node contract output arity mismatch".into()));
        }
        self.entries[index].as_mut().unwrap().outcome = outcome;
        Ok(())
    }
    /// Input descriptions for a contract (`None` for an unconnected optional
    /// input), or why a required one is not known. Edges are already validated.
    fn known_inputs(&self, e: &Entry) -> std::result::Result<Vec<Option<Description>>, Error> {
        e.inputs
            .iter()
            .zip(e.node.kind().inputs)
            .map(|(from, port)| match from {
                None if port.optional => Ok(None),
                None => Err(Error::MissingInput { node: e.node.kind().id, port: port.name }),
                Some(from) => match &self.entries[from.node.index].as_ref().unwrap().outcome {
                    Outcome::Known(outputs) => Ok(Some(outputs[from.index].clone())),
                    Outcome::Unknown(reason) => Err(reason.clone()),
                },
            })
            .collect()
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
            outcome: Outcome::Known(Vec::new()),
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
