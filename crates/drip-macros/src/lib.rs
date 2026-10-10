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
/// `contract` takes `&GlobalContext`, parameters and one Option<&Payload::Desc>
/// per input, and returns a tuple of Option<Payload::Desc> outputs. Rust checks
/// that signature. The kernel receives the same global configuration through
/// its `KernelContext`.
/// Rustdoc and `references = [(label, url), ...]` become node help.
/// Optional `name` and `category` default to the ID and `"processing"`.
/// The parameter type supplies its schema through `Parameters` (usually derived).
/// Use `&()` for nodes without parameters.
/// `Option<Read<Port>>` declares an optional input; output ports remain required.
/// `port_labels(image = "RGB", output = "RGB")` overrides displayed payload
/// labels by function argument name. Other labels use the declared payload name
/// (or Rust's type name for custom port wrappers). Labels do not rename ports
/// or change type/interpretation checks.
#[proc_macro_attribute]
pub fn node(args: TokenStream, item: TokenStream) -> TokenStream {
    node::expand(args.into(), parse_macro_input!(item as syn::ItemFn))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive editor ranges and documentation from typed parameter fields.
/// Flattening reuses a shared schema. Serde owns parameter persistence.
/// Labels default to sentence case; `#[label("Exposure (EV)")]` overrides them.
#[proc_macro_derive(Parameters, attributes(param, external, label))]
pub fn parameters(input: TokenStream) -> TokenStream {
    parameters::expand(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// A finite parameter choice. Each variant declares its persisted spelling
/// with `#[choice("name")]`. Generates JSON conversion and `variant.schema()`;
/// use that schema in `#[param(...)]` on a field of this enum type.
/// Variants may contain one parameter struct; its fields render only when selected.
/// Serialization uses Serde's externally tagged representation.
/// `#[label("sRGB")]` supplies display text independently of the persisted key.
#[proc_macro_derive(Choice, attributes(choice, label))]
pub fn choice(input: TokenStream) -> TokenStream {
    choice::expand(parse_macro_input!(input as syn::DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Discover a concrete GUI implementation beside its `GuiNode` impl.
/// Runtime adapters belong to drip-gui; this emits only the local binding entry.
#[proc_macro_attribute]
pub fn gui_node(args: TokenStream, input: TokenStream) -> TokenStream {
    gui_node::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
