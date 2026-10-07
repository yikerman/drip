//! Scope preparation and density reductions.

mod density;
mod histogram;
mod view;

pub use density::{Scope, ScopeAxes, vectorscope_xyz};
pub use histogram::Histogram;

#[cfg(test)]
mod tests;

fn logarithmic(scale: drip::nodes::scopes::Scale) -> bool {
    matches!(scale, drip::nodes::scopes::Scale::Log)
}
