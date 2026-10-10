//! Persisted enum variants and their parameter schemas.
use quote::quote;

pub(crate) fn expand(item: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let syn::Data::Enum(data) = &item.data else {
        return Err(syn::Error::new_spanned(&item, "Choice requires an enum"));
    };
    if !item.generics.params.is_empty() || data.variants.is_empty() {
        return Err(syn::Error::new_spanned(&item, "choices must be concrete and nonempty"));
    }
    let mut names = Vec::new();
    let mut schemas = Vec::new();
    let mut patterns = Vec::new();
    let mut serialize = Vec::new();
    let mut decode_variants = Vec::new();
    let mut decode_arms = Vec::new();
    let mut unique = std::collections::BTreeSet::new();
    let ty = &item.ident;
    for (index, variant) in data.variants.iter().enumerate() {
        let attrs: Vec<_> = variant.attrs.iter().filter(|a| a.path().is_ident("choice")).collect();
        if attrs.len() != 1 {
            return Err(syn::Error::new_spanned(variant, "expected one #[choice(\"name\")]"));
        }
        let name: syn::LitStr = attrs[0].parse_args()?;
        if !unique.insert(name.value()) {
            return Err(syn::Error::new_spanned(name, "duplicate persisted choice name"));
        }
        let label = crate::documentation::label(&variant.attrs, &name.value())?;
        let ident = &variant.ident;
        let index = index as u32;
        let params = match &variant.fields {
            syn::Fields::Unit => {
                patterns.push(quote!(Self::#ident));
                serialize.push(quote!(Self::#ident => serializer.serialize_unit_variant(stringify!(#ty), #index, #name)));
                decode_variants.push(quote!(#[serde(rename = #name)] #ident));
                decode_arms.push(quote!(Decoded::#ident => Self::#ident));
                quote!(None)
            }
            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let payload = &fields.unnamed[0].ty;
                patterns.push(quote!(Self::#ident(..)));
                serialize.push(quote!(Self::#ident(value) => serializer.serialize_newtype_variant(stringify!(#ty), #index, #name, value)));
                decode_variants.push(quote!(#[serde(rename = #name)] #ident(#payload)));
                decode_arms.push(quote!(Decoded::#ident(value) => Self::#ident(value)));
                quote!(Some(<#payload as ::drip::param::Parameters>::SPECS))
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "choice variants must be unit variants or contain one parameter struct",
                ));
            }
        };
        schemas.push(
            quote!(::drip::param::ChoiceSpec { name: #name, label: #label, parameters: #params }),
        );
        names.push(name);
    }
    Ok(quote! {
        impl #ty {
            pub const OPTIONS: &'static [&'static str] = &[#(#names),*];
            pub const CHOICES: &'static [::drip::param::ChoiceSpec] = &[#(#schemas),*];
            pub const fn as_str(&self) -> &'static str {
                match self { #(#patterns => #names),* }
            }
            pub const fn schema(&self) -> ::drip::param::ParamKind {
                ::drip::param::ParamKind::Choice { options: Self::CHOICES, default: self.as_str() }
            }
        }
        impl ::drip::param::ParameterField for #ty {
            const TYPE: ::drip::param::FieldType = ::drip::param::FieldType::Choice(Self::OPTIONS);
        }
        impl<'de> ::drip::__private::serde::Deserialize<'de> for #ty {
            fn deserialize<D: ::drip::__private::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                #[derive(::drip::__private::serde::Deserialize)]
                #[serde(crate = "::drip::__private::serde")]
                enum Decoded { #(#decode_variants),* }
                Ok(match <Decoded as ::drip::__private::serde::Deserialize>::deserialize(deserializer)? {
                    #(#decode_arms),*
                })
            }
        }
        impl ::drip::__private::serde::Serialize for #ty {
            fn serialize<S: ::drip::__private::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                match self { #(#serialize),* }
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
