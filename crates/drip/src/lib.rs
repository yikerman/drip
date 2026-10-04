//! Drip (Done Right Image Processing): all processing, the node graph, color
//! management and persistence. Frontends (`drip-cli`, `drip-gui`) are thin
//! layers over this crate; it must never depend on a GUI toolkit.

pub mod eval;
pub mod graph;
pub mod node;
pub mod param;
pub mod project;
pub mod resource;
pub mod value;
