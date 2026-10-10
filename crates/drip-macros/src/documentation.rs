pub(crate) fn documentation(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }
            let syn::Meta::NameValue(meta) = &attr.meta else {
                return None;
            };
            let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(value), .. }) = &meta.value else {
                return None;
            };
            Some(value.value().trim().to_owned())
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub(crate) fn label(attrs: &[syn::Attribute], key: &str) -> syn::Result<String> {
    if let Some(attr) = attrs.iter().find(|attr| attr.path().is_ident("label")) {
        return Ok(attr.parse_args::<syn::LitStr>()?.value());
    }
    let text = key.replace('_', " ");
    let mut chars = text.chars();
    Ok(chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect()))
}
