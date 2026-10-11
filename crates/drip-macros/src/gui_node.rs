//! GUI discovery only: register extras implemented on a node's `Definition`.
use proc_macro2::TokenStream;
use quote::quote;

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new(proc_macro2::Span::call_site(), "gui_node takes no options"));
    }
    let item: syn::ItemImpl = syn::parse2(input)?;
    if !matches!(item.self_ty.as_ref(), syn::Type::Path(_)) {
        return Err(syn::Error::new_spanned(&item.self_ty, "expected a node's Definition type"));
    }
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "GUI discovery requires a concrete type",
        ));
    }
    let ty = &item.self_ty;
    // Every node type is named `Definition`; an anonymous scope keeps entries apart.
    Ok(quote! {
        #item
        const _: () = {
            #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
            static BINDING: crate::node_ui::Binding = crate::node_ui::Binding::new::<#ty>();
        };
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_the_node_type_and_keeps_its_impl() {
        let input = quote!(impl GuiNode for histogram::observe::Definition {});
        let file: syn::File = syn::parse2(expand(quote!(), input.clone()).unwrap()).unwrap();
        let [syn::Item::Impl(implementation), syn::Item::Const(scope)] = file.items.as_slice()
        else {
            panic!("expected the original impl and its registration scope")
        };
        assert_eq!(quote!(#implementation).to_string(), input.to_string());
        let syn::Expr::Block(block) = scope.expr.as_ref() else { panic!("expected a block") };
        let [syn::Stmt::Item(syn::Item::Static(registration))] = block.block.stmts.as_slice()
        else {
            panic!("expected one registration")
        };
        let value = &registration.expr;
        assert_eq!(
            quote!(#value).to_string(),
            quote!(crate::node_ui::Binding::new::<histogram::observe::Definition>()).to_string()
        );
    }

    #[test]
    fn rejects_options_and_nonconcrete_gui_types() {
        for (args, item, expected) in [
            (
                quote!(name = "gui"),
                quote!(impl GuiNode for Definition {}),
                "gui_node takes no options",
            ),
            (
                quote!(),
                quote!(impl GuiNode for (Definition,) {}),
                "expected a node's Definition type",
            ),
            (
                quote!(),
                quote!(
                    impl<T> GuiNode for Definition<T> {}
                ),
                "GUI discovery requires a concrete type",
            ),
        ] {
            assert_eq!(expand(args, item).unwrap_err().to_string(), expected);
        }
    }
}
