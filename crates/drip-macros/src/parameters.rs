//! Parameter field schemas, independent of node declarations.
use quote::quote;

use crate::documentation;

pub(crate) fn expand(item: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let syn::Data::Struct(syn::DataStruct { fields: syn::Fields::Named(fields), .. }) = &item.data
    else {
        return Err(syn::Error::new_spanned(
            item,
            "Parameters requires a struct with named fields",
        ));
    };
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(item.generics, "parameter schemas must be concrete"));
    }
    let name = &item.ident;
    let mut groups = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        let attr = field.attrs.iter().find(|a| a.path().is_ident("param")).ok_or_else(|| {
            syn::Error::new_spanned(field, "missing #[param(kind)] or #[param(flatten)]")
        })?;
        let kind: syn::Expr = attr.parse_args()?;
        if matches!(&kind, syn::Expr::Path(p) if p.path.is_ident("flatten")) {
            groups.push(quote!(<#ty as ::drip::param::Parameters>::SPECS));
        } else {
            let key = ident.to_string();
            let label = crate::documentation::label(&field.attrs, &key)?;
            let external = field.attrs.iter().any(|attr| attr.path().is_ident("external"));
            let docs = documentation(&field.attrs);
            groups.push(quote! {
                &[{
                    assert!(
                        <#ty as ::drip::param::ParameterField>::TYPE.accepts(&(#kind)),
                        "parameter field type disagrees with schema",
                    );
                    ::drip::param::ParamSpec {
                        name: #key,
                        label: #label,
                        kind: #kind,
                        external: #external,
                        documentation: #docs,
                    }
                }]
            });
        }
    }
    Ok(quote! {
        impl ::drip::param::Parameters for #name {
            const SPECS: &'static [::drip::param::ParamSpec] =
                &::drip::param::concat_specs::<{0 #(+ (#groups).len())*}>(&[#(#groups),*]);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_external_fields_without_generating_a_parameter_decoder() {
        let item = syn::parse_quote! {
            struct Parameters {
                #[param(ParamKind::Bool { default: false })]
                fixed: bool,
                #[external]
                #[param(ParamKind::Bool { default: true })]
                input: bool,
            }
        };
        let implementation: syn::ItemImpl = syn::parse2(expand(item).unwrap()).unwrap();
        let [syn::ImplItem::Const(specs)] = implementation.items.as_slice() else {
            panic!("parameter schemas must contain only SPECS")
        };
        assert_eq!(specs.ident, "SPECS");
        let syn::Expr::Reference(specs) = &specs.expr else { panic!("expected borrowed specs") };
        let syn::Expr::Call(concat) = specs.expr.as_ref() else {
            panic!("expected concatenated specs")
        };
        let syn::Expr::Reference(groups) = &concat.args[0] else {
            panic!("expected borrowed groups")
        };
        let syn::Expr::Array(groups) = groups.expr.as_ref() else {
            panic!("expected schema groups")
        };
        let mut external_flags = Vec::new();
        for group in &groups.elems {
            let syn::Expr::Reference(group) = group else { panic!("expected borrowed group") };
            let syn::Expr::Array(group) = group.expr.as_ref() else {
                panic!("expected schema group")
            };
            let syn::Expr::Block(group) = &group.elems[0] else {
                panic!("expected checked schema")
            };
            let Some(syn::Stmt::Expr(syn::Expr::Struct(spec), _)) = group.block.stmts.last() else {
                panic!("expected parameter spec")
            };
            let external = spec
                .fields
                .iter()
                .find(
                    |field| matches!(&field.member, syn::Member::Named(name) if name == "external"),
                )
                .unwrap();
            let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(external), .. }) = &external.expr
            else {
                panic!("expected external flag")
            };
            external_flags.push(external.value);
        }
        assert_eq!(external_flags, [false, true]);
    }
}
