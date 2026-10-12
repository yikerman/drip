//! Undo history of whole-graph snapshots. Nodes are immutable and shared by
//! `Arc`, so a snapshot copies entries, edges and labels; a node a later step
//! replaced, with any file it loaded, lives while a step refers to it.

use std::collections::VecDeque;
use std::sync::LazyLock;

use crate::model::Graph;

const DEFAULT_DEPTH: usize = 100;

/// Undo steps kept, from `DRIP_HISTORY`.
static DEPTH: LazyLock<usize> = LazyLock::new(|| {
    let Ok(text) = std::env::var("DRIP_HISTORY") else { return DEFAULT_DEPTH };
    text.parse().unwrap_or_else(|_| {
        log::warn!("DRIP_HISTORY must be a step count, not {text:?}; keeping {DEFAULT_DEPTH}");
        DEFAULT_DEPTH
    })
});

#[derive(Default)]
pub struct History {
    undo: VecDeque<Graph>,
    redo: Vec<Graph>,
    /// The graph after the latest step.
    current: Graph,
}

impl History {
    pub fn new(graph: Graph) -> Self {
        History { current: graph, ..Self::default() }
    }

    /// Makes `graph` a step; steps undone before it are dropped.
    pub fn commit(&mut self, graph: &Graph) {
        self.undo.push_back(std::mem::replace(&mut self.current, graph.clone()));
        if self.undo.len() > *DEPTH {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// The graph before the latest step.
    pub fn undo(&mut self) -> Option<Graph> {
        let graph = self.undo.pop_back()?;
        self.redo.push(std::mem::replace(&mut self.current, graph.clone()));
        Some(graph)
    }

    /// The graph after the latest undone step.
    pub fn redo(&mut self) -> Option<Graph> {
        let graph = self.redo.pop()?;
        self.undo.push_back(std::mem::replace(&mut self.current, graph.clone()));
        Some(graph)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(names: &[&str]) -> Graph {
        let mut graph = Graph::default();
        for name in names {
            let id = graph.add_node(drip::definition::kind("tone.exposure").unwrap()).unwrap();
            graph.set_name(id, name).unwrap();
        }
        graph
    }

    fn names(graph: &Graph) -> Vec<String> {
        graph.nodes().map(|(_, node)| node.name).collect()
    }

    #[test]
    fn steps_undo_and_redo_until_a_new_step() {
        let mut history = History::new(graph(&[]));
        history.commit(&graph(&["a"]));
        history.commit(&graph(&["a", "b"]));
        assert_eq!(names(&history.undo().unwrap()), ["a"]);
        assert!(history.undo().unwrap().nodes().next().is_none());
        assert!(history.undo().is_none());
        assert_eq!(names(&history.redo().unwrap()), ["a"]);
        history.commit(&graph(&["c"]));
        assert!(!history.can_redo(), "a new step drops undone steps");
        assert_eq!(names(&history.undo().unwrap()), ["a"]);
    }

    #[test]
    fn keeps_the_latest_steps() {
        let mut history = History::new(Graph::default());
        for _ in 0..*DEPTH + 5 {
            history.commit(&Graph::default());
        }
        assert_eq!(std::iter::from_fn(|| history.undo()).count(), *DEPTH);
    }
}
