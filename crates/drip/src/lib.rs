//! Drip's headless library: a graph of image-processing nodes, evaluated on
//! demand on one CubeCL device.
//!
//! A node computes context × parameters × inputs → outputs. You declare one
//! as a Rust function with [`#[node]`](macro@node), connect nodes in a
//! [`Dag`](graph::Dag), and ask an [`Evaluator`](eval::Evaluator) for an
//! output. Evaluation runs only what the demand needs, fuses chains of
//! per-pixel and geometric steps into single kernels, and keeps what it
//! computed for the next demand.
//!
//! ```text
//!   #[node] fn ──► Kind: ports, contract, Shape        registry::NODES
//!                        │
//!   Dag::connect ────────┘  checks payloads, occupancy, cycles
//!       │
//!       ▼  demand: targets, scale, regions
//!   Evaluator ──key──► Store ──hit──► Image
//!       │ miss
//!       ├─ kernel, foreign node ─────────► Raster (host and device samples)
//!       └─ map, warp, resample, stencil ─► deferred Image ─► one fused kernel
//! ```
//!
//! The library has six parts:
//!
//! - **Declaring nodes.** [`definition`] holds [`Kind`](definition::Kind) and
//!   the [`Node`](definition::Node) trait that [`macro@node`] implements,
//!   [`param`] the parameter schemas, and [`registry`] the hand-kept lists of
//!   kinds and tags a frontend offers.
//! - **Graphs.** [`graph`] edits a [`Dag`](graph::Dag) and checks what ports
//!   alone determine. [`ports`] holds the typed port views a node function
//!   receives, and [`project`] persists graphs.
//! - **Values.** [`payload`] defines payload families, and [`image`] the
//!   [`Image`](image::Image) on an edge and the [`Raster`](image::Raster)
//!   that holds its samples on the host, the device or both.
//! - **Shapes and fusion.** [`shape`] says how evaluation runs each kind. The
//!   segment compiler (`plan.rs`, private) composes the deferred steps of
//!   maps, warps, resamples and stencils into one kernel per segment.
//! - **Evaluation.** [`eval`] forces demands, [`store`] keeps their values
//!   and runs foreign nodes, and [`expand`] builds the sub-graphs of meta
//!   nodes.
//! - **Nodes.** [`node`](mod@node) holds the processing nodes, their payloads and
//!   colour management. [`runtime`] selects the compute device.
//!
//! Contracts check declared compatibility, not physical accuracy.
//!
//! # Example
//!
//! Brighten a bound RGB image by one stop:
//!
//! ```no_run
//! use drip::{
//!     eval::Evaluator,
//!     graph::Dag,
//!     node::{self, data::*, exposure},
//!     runtime::RuntimeContext,
//! };
//!
//! let mut dag = Dag::new();
//! let desc = RgbDesc::new(Extent { width: 2, height: 1 }, Coordinates::Space(REC2020_D65));
//! let image = node::source::<Rgb>(&mut dag, desc, vec![[0.1, 0.2, 0.3]; 2])?;
//! let brighter = exposure::add(&mut dag, exposure::Exposure { ev: 1.0 })?;
//! dag.connect(image, brighter.image)?;
//!
//! let evaluator = Evaluator::new(RuntimeContext::from_env()?);
//! let result = evaluator.evaluate::<Rgb>(&dag, &Default::default(), brighter.output)?;
//! assert_eq!(result.data.host()?[0], [0.2, 0.4, 0.6]);
//! # Ok::<(), drip::Error>(())
//! ```
//!
//! The crate README walks through adding a node of each shape.

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

/// The README's examples, compiled by `cargo test --doc`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme {}
