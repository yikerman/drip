//! Parse a concrete kernel declaration, then generate its graph adapter.
use proc_macro2::TokenStream;
use syn::spanned::Spanned;

mod generate;
mod parse;

use parse::{Options, Signature};

pub(crate) fn expand(args: TokenStream, function: syn::ItemFn) -> syn::Result<TokenStream> {
    let options = Options::parse(args, function.sig.span())?;
    let signature = Signature::parse(&function.sig)?;
    if options.assets && signature.source {
        let message = "a source loads its outputs itself; remove assets";
        return Err(syn::Error::new(function.sig.ident.span(), message));
    }
    match (&options.contract, signature.outputs().next()) {
        (Some(contract), _) if signature.source => {
            let message = "a source describes its outputs by loading them; remove the contract";
            return Err(syn::Error::new(contract.span(), message));
        }
        (None, Some(output)) if !signature.source => {
            let message = "a node with outputs needs a contract";
            return Err(syn::Error::new(output.name.span(), message));
        }
        _ => {}
    }
    Ok(generate::node(&function, &options, &signature))
}

#[cfg(test)]
mod tests;
