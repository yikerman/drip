//! Meta nodes: kinds whose function builds a sub-graph of other kinds.
//!
//! Some operations are graphs of other operations whose size depends on
//! their parameters or input, such as a pyramid with one blur per level, as
//! vkdt's `create_nodes` builds them. A meta node's function wires a
//! [`SubGraph`] to its ports, and [`eval`](crate::eval) runs those nodes in
//! its place, so the graph a user edits keeps one node while each part is
//! keyed and stored like any node.
use crate::{
    Error, Result,
    definition::Kind,
    graph::{Dag, Entry},
    payload::Payload,
    ports::{Direction, Inlet, Input, Outlet, Output, PortId},
    shape::Shape,
};

/// The graph a meta node runs in its place. Its function adds nodes to
/// `dag` with their kinds' generated `add` and connects them with
/// [`Dag::connect`], then maps each of the meta node's ports with
/// [`input`](Self::input) and [`output`](Self::output).
pub struct SubGraph {
    pub dag: Dag,
    /// The meta node's kind, whose ports these are.
    kind: &'static Kind,
    /// Per input port of the meta node, the inputs of sub-graph nodes it
    /// feeds.
    inputs: Vec<Vec<PortId>>,
    /// Per output port of the meta node, the sub-graph output it is.
    outputs: Vec<Option<PortId>>,
}

/// Where an input of a sub-graph node reads from.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    /// An output of another node of the sub-graph.
    Node(PortId),
    /// The meta node's input of this index.
    Meta(usize),
}

impl SubGraph {
    #[doc(hidden)]
    pub fn new(kind: &'static Kind) -> Self {
        Self {
            dag: Dag::new(),
            kind,
            inputs: vec![Vec::new(); kind.inputs.len()],
            outputs: vec![None; kind.outputs.len()],
        }
    }

    /// Feed the meta node's input `inlet` to `to`, an input of a node of
    /// this sub-graph that nothing else feeds.
    pub fn input<P: Payload, R>(&mut self, inlet: Inlet<'_, P, R>, to: Input<P>) -> Result<()> {
        let entry = self.dag.input(to.id)?;
        if entry.inputs[to.id.index].is_some() || self.inputs.iter().flatten().any(|p| *p == to.id)
        {
            return Err(Error::Graph("input is already connected".into()));
        }
        self.inputs[inlet.index].push(to.id);
        Ok(())
    }

    /// Make `from`, an output of a node of this sub-graph, the meta node's
    /// output `outlet`.
    pub fn output<P: Payload>(&mut self, from: Output<P>, outlet: Outlet<P>) -> Result<()> {
        self.dag.output(from.id)?;
        let slot = &mut self.outputs[outlet.index];
        if slot.is_some() {
            return Err(Error::Graph("a meta output is set once".into()));
        }
        *slot = Some(from.id);
        Ok(())
    }

    /// This sub-graph once its function returns. Fails when an output of the
    /// meta node is unset, an input the meta node feeds is also connected
    /// inside, or a node is foreign or meta: foreign jobs belong to the
    /// store under their own keys, and expansions do not nest.
    #[doc(hidden)]
    pub fn finish(self) -> Result<Self> {
        let id = self.kind.id;
        if let Some(port) = self.outputs.iter().position(Option::is_none) {
            let name = self.kind.outputs[port].name;
            return Err(Error::Graph(format!("{id} leaves its output {name} unset")));
        }
        for &to in self.inputs.iter().flatten() {
            if self.dag.input(to)?.inputs[to.index].is_some() {
                return Err(Error::Graph(format!("{id} feeds an input that is connected")));
            }
        }
        for (_, node) in self.dag.nodes() {
            if matches!(node.kind().shape, Shape::Foreign(_) | Shape::Meta) {
                let kind = node.kind().id;
                return Err(Error::Graph(format!("{id} cannot expand to {kind}")));
            }
        }
        Ok(self)
    }

    /// The nodes in an order that runs every node after its inputs.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.dag.order.iter().filter_map(|&index| self.dag.entries[index].as_ref())
    }

    /// Where input `index` of `entry` reads from, `None` when nothing feeds
    /// it.
    pub(crate) fn source(&self, entry: &Entry, index: usize) -> Option<Source> {
        let port = PortId { node: entry.id, index, direction: Direction::Input };
        entry.inputs[index]
            .map(Source::Node)
            .or_else(|| self.inputs.iter().position(|fed| fed.contains(&port)).map(Source::Meta))
    }

    /// The sub-graph outputs in the meta node's port order.
    pub(crate) fn outputs(&self) -> impl Iterator<Item = PortId> + '_ {
        self.outputs.iter().flatten().copied()
    }
}
