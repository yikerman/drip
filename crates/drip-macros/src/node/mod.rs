//! Parse a concrete kernel declaration, then generate its graph adapter.
use proc_macro2::TokenStream;
use syn::spanned::Spanned;

mod generate;
mod parse;

use parse::{Options, Signature};

pub(crate) fn expand(args: TokenStream, function: syn::ItemFn) -> syn::Result<TokenStream> {
    let options = Options::parse(args, function.sig.span())?;
    let mut signature = Signature::parse(&function.sig)?;
    signature.label_ports(&options.port_labels)?;
    Ok(generate::node(&function, &options, &signature))
}

#[cfg(test)]
mod tests;
