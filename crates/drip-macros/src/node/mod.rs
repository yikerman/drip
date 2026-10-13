//! `#[node]`: parse a node function's declaration, then generate its graph
//! adapter.
use proc_macro2::TokenStream;
use syn::spanned::Spanned;

mod generate;
mod parse;

use parse::{Deferred, Family, Options, ShapeDecl, Signature};

pub(crate) fn expand(args: TokenStream, function: syn::ItemFn) -> syn::Result<TokenStream> {
    let options = Options::parse(args, function.sig.span())?;
    let signature = Signature::parse(&function.sig)?;
    let span = function.sig.ident.span();
    let foreign = matches!(options.shape, ShapeDecl::Foreign(_));
    if options.assets && foreign {
        let message = "a foreign node reads its files itself; remove assets";
        return Err(syn::Error::new(span, message));
    }
    // The declared shape and the port views must agree.
    let mismatch: Option<String> = match (&options.shape, signature.family) {
        (ShapeDecl::Kernel { .. }, Family::Context)
        | (ShapeDecl::Foreign(_), Family::Publish)
        | (ShapeDecl::Deferred(_), Family::Point)
        | (ShapeDecl::Meta, Family::Inlet) => None,
        (ShapeDecl::Foreign(_), _) => {
            Some("a foreign node publishes its outputs through Publish ports".into())
        }
        (_, Family::Publish) => Some("Publish ports declare foreign = memory or disk".into()),
        (ShapeDecl::Meta, _) => Some("a meta node wires Inlet and Outlet ports".into()),
        (_, Family::Inlet) => Some("Inlet and Outlet ports declare meta".into()),
        (ShapeDecl::Deferred(deferred), _) => {
            Some(format!("a {} declares its input and output as Point ports", deferred.name()))
        }
        (_, Family::Point) => Some(
            "Point ports declare map = body, warp, resample, reduce = BODY, bins = BODY or \
             stencil = BODY"
                .into(),
        ),
    };
    if let Some(message) = mismatch {
        return Err(syn::Error::new(span, message));
    }
    if let (ShapeDecl::Deferred(deferred), None) = (&options.shape, &signature.result) {
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
    let meta = matches!(options.shape, ShapeDecl::Meta);
    match (&options.contract, signature.outputs().next()) {
        (Some(contract), _) if foreign => {
            let message =
                "a foreign node describes its outputs by publishing them; remove the contract";
            return Err(syn::Error::new(contract.span(), message));
        }
        (Some(contract), _) if meta => {
            let message = "a meta node's sub-graph describes its outputs; remove the contract";
            return Err(syn::Error::new(contract.span(), message));
        }
        (None, Some(output)) if !foreign && !meta => {
            let message = "a node with outputs needs a contract";
            return Err(syn::Error::new(output.name.span(), message));
        }
        _ => {}
    }
    Ok(generate::node(&function, &options, &signature))
}

#[cfg(test)]
mod tests;
