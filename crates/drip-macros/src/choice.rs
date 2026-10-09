//! Persisted spellings and schemas for finite parameter choices.
use quote::quote;

pub(crate) fn expand(item: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let syn::Data::Enum(data) = &item.data else {
        return Err(syn::Error::new_spanned(&item, "Choice requires an enum"));
    };
    if !item.generics.params.is_empty() || data.variants.is_empty() {
        return Err(syn::Error::new_spanned(&item, "choices must be concrete and nonempty"));
    }
    let mut variants = Vec::new();
    let mut names = Vec::new();
    let mut unique = std::collections::BTreeSet::new();
    for variant in &data.variants {
        if !matches!(variant.fields, syn::Fields::Unit) {
            return Err(syn::Error::new_spanned(variant, "choice variants must be unit variants"));
        }
        let attrs: Vec<_> = variant.attrs.iter().filter(|a| a.path().is_ident("choice")).collect();
        if attrs.len() != 1 {
            return Err(syn::Error::new_spanned(variant, "expected one #[choice(\"name\")]"));
        }
        let name: syn::LitStr = attrs[0].parse_args()?;
        if !unique.insert(name.value()) {
            return Err(syn::Error::new_spanned(name, "duplicate persisted choice name"));
        }
        variants.push(&variant.ident);
        names.push(name);
    }
    let name = &item.ident;
    Ok(quote! {
        impl #name {
            pub const OPTIONS: &'static [&'static str] = &[#(#names),*];

            pub const fn as_str(&self) -> &'static str {
                match self { #(Self::#variants => #names),* }
            }
            pub const fn schema(&self) -> ::drip::param::ParamKind {
                ::drip::param::ParamKind::Choice {
                    options: Self::OPTIONS,
                    default: self.as_str(),
                }
            }
        }
        impl ::drip::param::ParameterField for #name {
            const TYPE: ::drip::param::FieldType = ::drip::param::FieldType::Choice(Self::OPTIONS);
        }
        impl<'de> ::drip::__private::serde::Deserialize<'de> for #name {
            fn deserialize<D: ::drip::__private::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                let value = <String as ::drip::__private::serde::Deserialize>::deserialize(deserializer)?;
                match value.as_str() {
                    #(#names => Ok(Self::#variants),)*
                    _ => Err(::drip::__private::serde::de::Error::unknown_variant(
                        &value,
                        Self::OPTIONS,
                    )),
                }
            }
        }
        impl ::drip::__private::serde::Serialize for #name {
            fn serialize<S: ::drip::__private::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_return_types_cannot_resolve_to_a_callers_result_alias() {
        let item = syn::parse_quote! {
            enum Choice {
                #[choice("one")]
                One,
            }
        };
        let generated: syn::File = syn::parse2(expand(item).unwrap()).unwrap();
        let mut checked = Vec::new();
        for item in generated.items {
            let syn::Item::Impl(implementation) = item else {
                continue;
            };
            for item in implementation.items {
                let syn::ImplItem::Fn(method) = item else {
                    continue;
                };
                if method.sig.ident != "serialize" && method.sig.ident != "deserialize" {
                    continue;
                }
                let syn::ReturnType::Type(_, ty) = method.sig.output else {
                    panic!("expected a serde result")
                };
                let syn::Type::Path(result) = ty.as_ref() else {
                    panic!("expected a qualified result type")
                };
                assert!(result.path.leading_colon.is_some());
                let path: Vec<_> =
                    result.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
                assert_eq!(path, ["core", "result", "Result"]);
                checked.push(method.sig.ident.to_string());
            }
        }
        assert_eq!(checked, ["deserialize", "serialize"]);
    }
}
