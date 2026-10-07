//! Applies frontend graph edits and owns their redraw/evaluation effects.

use crate::node_ui::{Part, Popped, ports};
use crate::render::node_views::Drawable;
use crate::ui_state::LayoutField;
use crate::worker::ViewResult;
use drip::graph::{Graph, GraphError, Node, NodeId, Port};
use drip::node::NodeKind;
use egui::Vec2;
use serde_json::Value as Json;
use std::collections::BTreeSet;

/// What one frame of node GUIs reads from the app, and what they did.
pub struct Frame<'a> {
    pub results: &'a dyn Fn(NodeId) -> Option<&'a ViewResult>,
    /// The windows popped out.
    pub popped: &'a BTreeSet<Popped>,
    /// Disables actions while one runs.
    pub action_running: bool,
    pub report: Report,
}

/// What node GUIs did in one frame, for the app to apply.
#[derive(Default)]
pub struct Report {
    /// Whether processing inputs or connections changed.
    pub edited: bool,
    /// Whether shared presentation changed, including noncomputational edits.
    pub redraw: bool,
    /// An edit the graph refused.
    pub refused: Option<String>,
    /// An action the user asked to run.
    pub action: Option<(NodeId, &'static str)>,
    /// A window the user opened or closed.
    pub toggled: Option<Popped>,
}

/// All frontend graph mutations pass through `Frame::edit`, which owns their
/// redraw and evaluation effects. Windows read the same graph after each edit.
pub enum Edit<'a> {
    Add(&'static NodeKind, Vec2),
    Remove(NodeId),
    Connect(Port, Port),
    Disconnect(Port),
    Param(NodeId, &'a str, Json),
    Name(NodeId, &'a str),
    External(NodeId, &'a str, bool),
    Ui(NodeId, LayoutField, Vec2),
}

impl<'a> Frame<'a> {
    pub fn new(
        results: &'a dyn Fn(NodeId) -> Option<&'a ViewResult>,
        popped: &'a BTreeSet<Popped>,
        action_running: bool,
    ) -> Self {
        Frame { results, popped, action_running, report: Report::default() }
    }

    /// Applies an edit immediately; adding a node returns its id for selection.
    pub fn edit(&mut self, graph: &mut Graph, edit: Edit<'_>) -> Option<NodeId> {
        let evaluate = !matches!(edit, Edit::Name(..) | Edit::External(..) | Edit::Ui(..));
        let result: Result<_, GraphError> = (|| {
            match edit {
                Edit::Add(kind, pos) => {
                    let id = graph.add_node(kind);
                    set_ui(graph, id, LayoutField::Position, pos);
                    return Ok(Some(id));
                }
                Edit::Remove(id) => {
                    graph.remove_node(id);
                }
                Edit::Connect(output, input) => graph.connect(output, input)?,
                Edit::Disconnect(input) => {
                    graph.disconnect(&input);
                }
                Edit::Param(id, name, value) => graph.set_param(id, name, value)?,
                Edit::Name(id, name) => graph.set_name(id, name)?,
                Edit::External(id, name, external) => graph.set_external(id, name, external)?,
                Edit::Ui(id, key, value) => set_ui(graph, id, key, value),
            }
            Ok(None)
        })();
        match result {
            Ok(added) => {
                self.report.redraw = true;
                self.report.edited |= evaluate;
                added
            }
            Err(e) => {
                self.report.refused = Some(refusal(graph, &e));
                None
            }
        }
    }
}

/// One node as its GUI sees it. Edits go through the graph's validated
/// operations and are reported to the frame.
pub struct NodeCx<'g, 'f, 'a> {
    id: NodeId,
    graph: &'g mut Graph,
    frame: &'f mut Frame<'a>,
}

impl<'g, 'f, 'a> NodeCx<'g, 'f, 'a> {
    pub fn new(graph: &'g mut Graph, id: NodeId, frame: &'f mut Frame<'a>) -> Self {
        NodeCx { id, graph, frame }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn graph(&self) -> &Graph {
        self.graph
    }

    pub fn node(&self) -> &Node {
        self.graph.node(self.id).expect("GUIs are shown for existing nodes")
    }

    /// The view of the node's latest result, if it has one.
    pub fn view(&self) -> Option<&'a dyn Drawable> {
        (self.frame.results)(self.id)?.as_ref().ok()?.as_deref()
    }

    pub fn set_param(&mut self, name: &str, value: Json) {
        self.frame.edit(self.graph, Edit::Param(self.id, name, value));
    }

    pub fn set_name(&mut self, name: &str) {
        self.frame.edit(self.graph, Edit::Name(self.id, name));
    }

    pub fn set_external(&mut self, name: &str, external: bool) {
        self.frame.edit(self.graph, Edit::External(self.id, name, external));
    }

    /// Updates layout without changing processing parameters.
    pub fn set_ui(&mut self, field: LayoutField, value: Vec2) {
        self.frame.edit(self.graph, Edit::Ui(self.id, field, value));
    }

    pub fn run(&mut self, action: &'static str) {
        self.frame.report.action = Some((self.id, action));
    }

    /// Whether actions are disabled because one runs.
    pub fn action_running(&self) -> bool {
        self.frame.action_running
    }

    /// Whether `part` of the node is shown in its own window.
    pub fn popped(&self, part: Part) -> bool {
        self.frame.popped.contains(&Popped { node: self.id, part })
    }

    /// Opens the window of `part`, or closes it if open.
    pub fn toggle(&mut self, part: Part) {
        self.frame.report.toggled = Some(Popped { node: self.id, part });
    }
}

fn set_ui(graph: &mut Graph, id: NodeId, field: LayoutField, value: Vec2) {
    let mut ui = graph.node(id).expect("node exists").ui.clone();
    field.write(&mut ui, value);
    graph.set_ui(id, ui).expect("node exists");
}

/// Names the refused input in node terms. Connecting rechecks descendants,
/// so the input may lie downstream of the new edge.
fn refusal(graph: &Graph, error: &GraphError) -> String {
    match error {
        GraphError::TypeMismatch { input, mismatch } => format!(
            "{} · {} requires {}, got {} ({})",
            graph.node(input.0).expect("checked input").name,
            input.1,
            ports::name(mismatch.expected),
            ports::name(mismatch.actual),
            mismatch.reason,
        ),
        error => error.to_string(),
    }
}
