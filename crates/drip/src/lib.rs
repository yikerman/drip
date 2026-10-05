//! Drip (Done Right Image Processing): processing, color management and an editable
//! node graph, independent of any GUI.
//!
//! # Design intent
//!
//! Make the pipeline explicit while rejecting connections with incompatible data
//! semantics. Nodes request the weakest capability their mathematics needs;
//! their input/output tuples drive both connection checks and evaluation.
//! Rust checks signatures; documented trait laws describe what implementers must
//! uphold. Pure kernels and immutable results allow dependency-based cache reuse.
//!
//! # Reading order
//!
//! 1. [`image`]: data structures, capability hierarchy and semantic laws.
//! 2. [`value`] and [`ports`]: capability registration and tuple adapters that
//!    connect typed kernels to a heterogeneous graph.
//! 3. [`node`] and [`nodes`]: signatures, processing and explicit actions. Read
//!    `nodes/exposure.rs` for a small node, `nodes/scopes.rs` for capability inputs,
//!    and `nodes/export.rs` for file-writing actions.
//! 4. [`graph`]: nodes, named edges, connection validation and DAG traversal.
//! 5. [`eval`]: evaluation and node-result caching. Follow
//!    [`eval::Evaluator::evaluate`], then `Evaluator::run` and `Cache::stamp` in
//!    the source. [`resource`] separately caches decoded files shared by preview
//!    and full-detail actions; its lifetime differs from cached node results.
//! 6. [`view`]: headless presentation. [`param`], [`project`] and [`templates`]:
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
pub mod view;
