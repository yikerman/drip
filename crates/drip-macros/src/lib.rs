//! The procedural macros behind drip's node declarations.
//!
//! - [`macro@node`] turns a function into a node kind: its `Kind`, typed ports
//!   and the adapter evaluation calls.
//! - [`Parameters`] derives a parameter struct's defaults and schema from one
//!   typed declaration per field.
//! - [`Choice`] derives an enum's persisted spelling and schema.
//!
//! Use them through `drip`, which re-exports all three. The generated code
//! names `::drip` paths. The drip crate README has a compiled example of each
//! node shape.
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod choice;
mod documentation;
mod node;
mod parameters;

use documentation::documentation;

/// Declare a node from a function.
///
/// Generates a module named after the function holding the node's static
/// `KIND`, `add`, the typed `Ports`, `Inputs` and `Outputs`, and the adapter
/// that implements `drip::definition::Node`. The function's Rustdoc becomes
/// the node's help.
///
/// # Arguments
///
/// - `id = "..."`: the persistence key, in kebab case. Keep it once projects
///   use it.
/// - `name = "..."`: the display label, the ID by default.
/// - `tags = [tag::LOOK, ...]`: `Tag` statics from `drip::registry::tag`.
/// - `contract = f`: `fn(&GlobalContext, &Params, &Desc...) -> Result<(Desc, ...)>`,
///   with `Option<&Desc>` for an optional input, `None` when unconnected. It
///   describes every output and is required exactly when the node has
///   outputs and is neither foreign nor meta. Evaluation calls it before the
///   node runs.
/// - `version = N`: 1 by default. Bump it when the node computes something
///   else from the same parameters and inputs, so stored outputs no longer
///   match.
/// - `references = [(label, url), ...]`: sources shown with the node's help.
/// - `scaled`: a kernel that produces its outputs at the requested scale
///   itself, as demosaic does. Its contract and function receive the
///   request's `GlobalContext`, and its outputs are keyed by the scale. Every
///   other kernel receives scale 1, and evaluation reduces the images on a
///   plane it reads.
/// - `action = f`: `fn(&Params, &InputValues) -> Result<()>`, run on the
///   evaluated inputs, with `&Loaded` after the parameters when the node
///   declares `assets`. It binds the inputs whole, or by region when an input
///   exceeds the device budget.
/// - `assets`: the parameters implement `drip::definition::Assets`.
///
/// # Shapes
///
/// The remaining argument picks the shape and, with it, the signature:
///
/// | Argument | Function | Returns |
/// |---|---|---|
/// | none (kernel) | `fn(&KernelContext, &Params, Read..., Write... or Forward...)` | `Result<()>` |
/// | `foreign = memory` or `foreign = disk` | `fn(&Params, Read..., Publish...)` | `Result<()>` |
/// | `map = body` | `fn(&Params, Point<In>, Read..., Point<Out>)` | `Result<[f32; N]>` |
/// | `warp` | as for a map | `Result<Warp>` |
/// | `resample` | `fn(&Params, Point<In>, Point<Out>)` | `Result<&'static FilterBody>` |
/// | `reduce = BODY` | as for a map | `Result<[f32; N]>` |
/// | `bins = BODY` | as for a map | `Result<[f32; N]>` |
/// | `stencil = BODY` | as for a map | `Result<Stencil<N>>` |
/// | `meta` | `fn(&Params, &mut SubGraph, Inlet..., Outlet...)` | `Result<()>` |
///
/// Use `&()` for a node without parameters. `Option<Read<P>>` declares an
/// optional input, and output ports are required. A port may name a
/// requirement, `Read<'_, Rgb, Rec2020>`, checked on its description before
/// the contract for an input and after it for an output. An unread port
/// `_image` is declared as `image`.
///
/// A map's `body` is `#[cube] fn(px: Pixel, k: &[f32]) -> Pixel`, reading the
/// `N` constants the function returns. `BODY` names a `ReduceBody`,
/// `BinsBody` or `StencilBody` static. A foreign node runs on a thread of its
/// own with host inputs, once per key, and `disk` requires every output
/// payload to implement `Persist`.
#[proc_macro_attribute]
pub fn node(args: TokenStream, item: TokenStream) -> TokenStream {
    node::expand(args.into(), parse_macro_input!(item as syn::ItemFn))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive `Default` and the parameter schema from typed fields. Serde owns
/// persistence, and the typed default is the source of both `Default` and
/// the schema's default. Field attributes:
///
/// - `#[param(default = 1.5, range = 0.5..=4.0)]` for f32, f64 and i64, and
///   `#[param(default = true)]` for bool. Ranges guide controls only.
/// - `#[param(default = Choice::Variant)]`, or a bare `#[param]` for the
///   choice's own `#[default]` variant.
/// - `#[param(open = ["icc"])]` or `save = [..]` for an `Option<PathBuf>` file,
///   with the extensions dialogs offer.
/// - `#[param(flatten)]` reuses a nested schema, validated with its parent.
/// - `#[label("Exposure (EV)")]` replaces the sentence-case label.
/// - `#[external]` makes the field a template input by default.
///
/// `#[validate(path)]` on the struct names `fn(&Self) -> Result<()>`, checked
/// when parameters are admitted to a node.
#[proc_macro_derive(Parameters, attributes(param, external, label, validate))]
pub fn parameters(input: TokenStream) -> TokenStream {
    parameters::expand(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive a finite parameter choice: JSON conversion, `variant.schema()` and
/// `Default`. Each variant declares its persisted spelling with
/// `#[choice("name")]` and may hold one parameter struct, whose fields render
/// and persist only while the variant is selected. Serialization is Serde's
/// externally tagged form.
///
/// Exactly one `#[default]` variant supplies `Default`, using its struct's own
/// default, and `DEFAULT_SCHEMA`, which a bare `#[param]` field uses.
/// `#[label("sRGB")]` sets display text apart from the persisted key.
#[proc_macro_derive(Choice, attributes(choice, label, default))]
pub fn choice(input: TokenStream) -> TokenStream {
    choice::expand(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
