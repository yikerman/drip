//! Headless typed image processing with explicit host/device boundaries.
//!
//! Payloads describe concrete storage and carry domain-specific meaning. Nodes
//! validate runtime refinements; the DAG validates connections. Request-driven
//! evaluation shares transfers and retires intermediates without a result cache.
//! Numerical image kernels use WGSL through wgpu, including software adapters.
//! Loading, pure processing, and external actions remain separate.

pub mod color;
pub mod compute;
pub mod eval;
pub mod graph;
pub mod image;
pub mod node;
pub mod nodes;
pub mod param;
pub mod ports;
pub mod profile;
pub mod project;
pub mod resource;
pub mod templates;
pub mod value;

extern crate self as drip;
pub use drip_macros::{Choice, Parameters, node};

#[doc(hidden)]
pub mod __private {
    pub use linkme;
    pub use serde;
}
