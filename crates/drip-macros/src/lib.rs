//! Mechanical adapters for concrete nodes, parameter schemas and choices.
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod choice;
mod documentation;
mod gui_node;
mod node;
mod parameters;

use documentation::documentation;

/// Declare `fn(context, &parameters, Read<Port>..., Write<Port>...) -> Result<()>`.
/// A port may name a requirement, `Read<'_, Device<Rgb>, Rec2020>`, checked on
/// its description before the contract (inputs) or after it (outputs).
/// `contract` takes `&GlobalContext`, parameters and one known description per
/// input (`&Desc`, or `Option<&Desc>` for an optional input, `None` when
/// unconnected) and returns a tuple of output descriptions. It is required
/// exactly when the node has outputs. Unknown upstream descriptions skip it.
/// Rustdoc and `references = [(label, url), ...]` become node help.
/// Optional `name` and `category` default to the ID and `"processing"`.
/// The parameter type supplies its schema through `Parameters` (usually derived).
/// Use `&()` for nodes without parameters.
/// `Option<Read<Port>>` declares an optional input; output ports remain required.
/// A node without computation names an unread port `_image`; the port is `image`.
#[proc_macro_attribute]
pub fn node(args: TokenStream, item: TokenStream) -> TokenStream {
    node::expand(args.into(), parse_macro_input!(item as syn::ItemFn))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive `Default`, schemas and documentation from typed parameter fields:
/// `#[param(default = 1.5, range = 0.5..=4.0)]` for f32/f64/i64,
/// `#[param(default = true)]`, `#[param(default = Choice::Variant)]` or a bare
/// `#[param]` for the choice's own default, `#[param(open = ["icc"])]` or
/// `save = [..]` for `Option<PathBuf>` files, and `#[param(flatten)]` to reuse
/// a nested schema (validated with its parent). Serde owns persistence.
/// Labels default to sentence case; `#[label("Exposure (EV)")]` overrides them.
/// `#[validate(path)]` on the struct names `fn(&Self) -> Result<()>`, checked
/// when parameters are admitted to a node.
#[proc_macro_derive(Parameters, attributes(param, external, label, validate))]
pub fn parameters(input: TokenStream) -> TokenStream {
    parameters::expand(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A finite parameter choice. Each variant declares its persisted spelling
/// with `#[choice("name")]`. Generates JSON conversion and `variant.schema()`.
/// Exactly one `#[default]` variant generates `Default` (a variant's struct uses
/// its own default) and `DEFAULT_SCHEMA`, used by a bare `#[param]` field; a
/// field states `default = Choice::Variant` only to override it.
/// Variants may contain one parameter struct; its fields render only when selected.
/// Serialization uses Serde's externally tagged representation.
/// `#[label("sRGB")]` supplies display text independently of the persisted key.
#[proc_macro_derive(Choice, attributes(choice, label, default))]
pub fn choice(input: TokenStream) -> TokenStream {
    choice::expand(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Register a `GuiNode` implemented on a node's generated `Definition`, which
/// supplies its kind, parameters and typed inputs. Runtime adapters belong to
/// drip-gui; this emits only the local binding entry.
#[proc_macro_attribute]
pub fn gui_node(args: TokenStream, input: TokenStream) -> TokenStream {
    gui_node::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
