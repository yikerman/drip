//! Parse a concrete kernel declaration, then generate its graph adapter.
use proc_macro2::TokenStream;
use syn::spanned::Spanned;

mod generate;
mod parse;

use parse::{Deferred, Options, Signature};

pub(crate) fn expand(args: TokenStream, function: syn::ItemFn) -> syn::Result<TokenStream> {
    let options = Options::parse(args, function.sig.span())?;
    let signature = Signature::parse(&function.sig)?;
    let span = function.sig.ident.span();
    if options.assets && signature.foreign {
        let message = "a foreign node reads its files itself; remove assets";
        return Err(syn::Error::new(span, message));
    }
    match (options.foreign, signature.foreign) {
        (Some(_), false) => {
            let message = "a foreign node publishes its outputs through Publish ports";
            return Err(syn::Error::new(span, message));
        }
        (None, true) => {
            let message = "Publish ports declare foreign = memory or disk";
            return Err(syn::Error::new(span, message));
        }
        _ => {}
    }
    match (&options.deferred, signature.point) {
        (Some(deferred), false) => {
            let message =
                format!("a {} declares its input and output as Point ports", deferred.name());
            return Err(syn::Error::new(span, message));
        }
        (None, true) => {
            let message = "Point ports declare map = body, warp, resample, reduce = BODY, bins = BODY or \
                 stencil = BODY";
            return Err(syn::Error::new(span, message));
        }
        (Some(deferred), true) if signature.result.is_none() => {
            let message = match deferred {
                Deferred::Map(_) => "a map returns Result<[f32; N]>, the constants of its body",
                Deferred::Warp => "a warp returns Result<Warp>",
                Deferred::Resample => "a resample returns Result<&'static FilterBody>",
                Deferred::Reduce(_) | Deferred::Bins(_) => {
                    "a reduce or bins node returns Result<[f32; N]>, the constants of its body"
                }
                Deferred::Stencil(_) => {
                    "a stencil returns Result<Stencil<N>>, its radius and the constants of its body"
                }
            };
            return Err(syn::Error::new(function.sig.output.span(), message));
        }
        _ => {}
    }
    match (&options.contract, signature.outputs().next()) {
        (Some(contract), _) if signature.foreign => {
            let message =
                "a foreign node describes its outputs by publishing them; remove the contract";
            return Err(syn::Error::new(contract.span(), message));
        }
        (None, Some(output)) if !signature.foreign => {
            let message = "a node with outputs needs a contract";
            return Err(syn::Error::new(output.name.span(), message));
        }
        _ => {}
    }
    Ok(generate::node(&function, &options, &signature))
}

#[cfg(test)]
mod tests;
