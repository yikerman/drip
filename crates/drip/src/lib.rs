//! Concrete port values, CPU contracts, and demand-driven CubeCL execution.
//!
//! Graph edits check ports only. Evaluation runs the contracts of the nodes it
//! forces and hands each node its input images; a node asks them for host or
//! device samples and writes its outputs where it computes them. Map, warp and
//! resample nodes publish deferred images instead, and a segment of them runs
//! as one kernel, and a stencil runs with the maps after it. A meta node
//! runs a sub-graph of other kinds in its place. Foreign nodes run host
//! functions on threads of their own, and the evaluator's store keeps their
//! outputs, on disk when declared.
//! Passing a contract establishes declared compatibility, not physical accuracy.

extern crate self as drip;

pub mod definition;
pub mod eval;
pub mod expand;
pub mod graph;
pub mod image;
pub mod node;
pub mod param;
pub mod payload;
mod plan;
pub mod ports;
pub mod project;
pub mod registry;
pub mod runtime;
pub mod shape;
pub mod store;

pub use drip_macros::{Choice, Parameters, node};
pub use error::{Error, Result};

mod error {
    #[derive(Debug, Clone, PartialEq, thiserror::Error)]
    pub enum Error {
        #[error("{0}")]
        Contract(String),
        #[error("{0}")]
        Graph(String),
        #[error("node {node}: missing input {port}")]
        MissingInput { node: &'static str, port: &'static str },
        #[error("{0}")]
        Runtime(String),
        #[error("node {kind} ({node:?}): {source}")]
        Node { node: crate::ports::NodeId, kind: &'static str, source: Box<Error> },
        /// A foreign node's job is still running, and the evaluation was
        /// asked not to wait ([`Policy::Defer`](crate::eval::Policy::Defer)).
        #[error("being computed")]
        Pending,
    }
    impl From<String> for Error {
        fn from(s: String) -> Self {
            Self::Runtime(s)
        }
    }
    impl From<&str> for Error {
        fn from(s: &str) -> Self {
            Self::Runtime(s.into())
        }
    }
    pub type Result<T> = std::result::Result<T, Error>;
}

#[doc(hidden)]
pub mod __private {
    pub use serde;
    pub use serde_json;
}
