//! GUI discovery only; the frontend defines and runs the binding adapter.
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new(proc_macro2::Span::call_site(), "gui_node takes no options"));
    }
    let item: syn::ItemImpl = syn::parse2(input)?;
    let syn::Type::Path(path) = item.self_ty.as_ref() else {
        return Err(syn::Error::new_spanned(&item.self_ty, "expected a concrete GUI type"));
    };
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "GUI discovery requires a concrete type",
        ));
    }
    let ty = &item.self_ty;
    let name = &path.path.segments.last().expect("type path").ident;
    let entry = format_ident!("__DRIP_GUI_{}", name.to_string().to_uppercase());
    Ok(quote! {
        #item
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static #entry: crate::node_ui::Binding = crate::node_ui::Binding::new::<#ty>();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_the_concrete_gui_type_and_keeps_its_impl() {
        let input = quote!(impl GuiNode for preview::ImageGui {});
        let file: syn::File = syn::parse2(expand(quote!(), input.clone()).unwrap()).unwrap();
        let [syn::Item::Impl(implementation), syn::Item::Static(registration)] =
            file.items.as_slice()
        else {
            panic!("expected the original impl and its registration")
        };
        assert_eq!(quote!(#implementation).to_string(), input.to_string());
        assert_eq!(registration.ident, "__DRIP_GUI_IMAGEGUI");
        let ty = &registration.ty;
        let value = &registration.expr;
        assert_eq!(quote!(#ty).to_string(), quote!(crate::node_ui::Binding).to_string());
        assert_eq!(
            quote!(#value).to_string(),
            quote!(crate::node_ui::Binding::new::<preview::ImageGui>()).to_string()
        );
        assert_eq!(registration.attrs.len(), 1);
        let attribute = &registration.attrs[0];
        assert_eq!(
            quote!(#attribute).to_string(),
            quote!(#[linkme::distributed_slice(crate::node_ui::BINDINGS)]).to_string()
        );
    }

    #[test]
    fn rejects_options_and_nonconcrete_gui_types() {
        for (args, item, expected) in [
            (
                quote!(name = "gui"),
                quote!(impl GuiNode for ImageGui {}),
                "gui_node takes no options",
            ),
            (quote!(), quote!(impl GuiNode for (ImageGui,) {}), "expected a concrete GUI type"),
            (
                quote!(),
                quote!(
                    impl<T> GuiNode for ImageGui<T> {}
                ),
                "GUI discovery requires a concrete type",
            ),
        ] {
            assert_eq!(expand(args, item).unwrap_err().to_string(), expected);
        }
    }
}
