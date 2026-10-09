//! Concrete port values, CPU contracts, and demand-driven CubeCL execution.
//!
//! Graph edits validate descriptions before committing. Evaluation supplies
//! each node's declared CPU/device representations and allocates fresh outputs.
//! Passing a contract establishes declared compatibility, not physical accuracy.

extern crate self as drip;

pub mod definition;
pub mod eval;
pub mod graph;
pub mod node;
pub mod param;
pub mod payload;
pub mod ports;
pub mod project;
pub mod resource;
pub mod runtime;

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
    pub use linkme;
    pub use serde;
    pub use serde_json;
}
