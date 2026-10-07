//! Drip (Done Right Image Processing): processing, color management and an editable
//! node graph, independent of any GUI.
//!
//! # Design intent
//!
//! Make the pipeline explicit while rejecting connections with incompatible data
//! representations. Ordinary RGB nodes use linear Rec.2020/D65, including after
//! creative nonlinear processing. Concrete input/output tuples drive connection
//! checks and execution; node-local checks express actual-value prerequisites.
//! Rust checks signatures, not photographic intent or numerical correctness.
//! Pure kernels and immutable results allow dependency-based cache reuse.
//!
//! # Reading order
//!
//! 1. [`image`]: storage and concrete image interpretations.
//! 2. [`value`] and [`ports`]: concrete type descriptors and tuple adapters that
//!    connect typed kernels to a heterogeneous graph.
//! 3. [`mod@node`] and [`nodes`]: signatures, processing and explicit actions. Read
//!    `nodes/exposure.rs` for a working-RGB input, `nodes/preview.rs` for a declaration,
//!    and `nodes/export.rs` for file-writing actions.
//! 4. [`graph`]: nodes, named edges, connection validation and DAG traversal.
//! 5. [`eval`]: evaluation and node-result caching. Follow
//!    [`eval::Evaluator::evaluate`], then `Evaluator::run` and `Cache::stamp` in
//!    the source. [`resource`] separately caches decoded files shared by preview
//!    and full-detail actions; its lifetime differs from cached node results.
//! 6. [`param`], [`project`] and [`templates`]:
//!    validation and persistence. [`color`] and [`profile`]: color math and ICC.

pub mod color;
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
