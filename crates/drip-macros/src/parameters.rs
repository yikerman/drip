//! Parameter schemas and defaults from one typed declaration per field.
use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;

use crate::documentation;

/// What a field's `#[param(...)]` declares. The typed default is the source of
/// both `Default` and the schema's default value.
enum Field {
    Flatten,
    Number {
        default: syn::Expr,
        range: syn::ExprRange,
    },
    Bool {
        default: syn::Expr,
    },
    /// A `Choice` enum, with an explicit unit-variant default or its own.
    Choice {
        default: Option<syn::Expr>,
    },
    Path {
        output: bool,
        extensions: syn::ExprArray,
    },
}

impl Field {
    fn parse(attr: &syn::Attribute, ty: &syn::Type) -> syn::Result<Self> {
        if matches!(attr.meta, syn::Meta::Path(_)) {
            return Ok(Self::Choice { default: None });
        }
        let entries = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        if let [syn::Meta::Path(path)] = entries.iter().collect::<Vec<_>>().as_slice()
            && path.is_ident("flatten")
        {
            return Ok(Self::Flatten);
        }
        let (mut default, mut range, mut path) = (None, None, None);
        for entry in entries {
            let syn::Meta::NameValue(entry) = entry else {
                return Err(syn::Error::new(entry.span(), "expected key = value or flatten"));
            };
            let key = entry.path.get_ident().map(ToString::to_string).unwrap_or_default();
            let duplicate = || syn::Error::new(entry.path.span(), "duplicate parameter key");
            match key.as_str() {
                "default" if default.is_none() => default = Some(entry.value),
                "range" if range.is_none() => match entry.value {
                    syn::Expr::Range(r) if r.start.is_some() && r.end.is_some() => range = Some(r),
                    other => return Err(syn::Error::new(other.span(), "expected min..=max")),
                },
                "open" | "save" if path.is_none() => match entry.value {
                    syn::Expr::Array(extensions) => path = Some((key == "save", extensions)),
                    other => return Err(syn::Error::new(other.span(), "expected [\"ext\", ...]")),
                },
                "default" | "range" | "open" | "save" => return Err(duplicate()),
                _ => return Err(syn::Error::new(entry.path.span(), "unknown parameter key")),
            }
        }
        match (default, range, path) {
            (None, None, Some((output, extensions))) => Ok(Self::Path { output, extensions }),
            (Some(default), Some(range), None) => Ok(Self::Number { default, range }),
            (Some(default), None, None) if is(ty, "bool") => Ok(Self::Bool { default }),
            (default @ Some(_), None, None) => Ok(Self::Choice { default }),
            _ => Err(syn::Error::new(
                attr.span(),
                "expected default and range, a default, open/save extensions, or flatten",
            )),
        }
    }

    fn default(&self, ty: &syn::Type) -> TokenStream {
        match self {
            Self::Number { default, .. } | Self::Bool { default } => quote!(#default),
            Self::Choice { default: Some(default) } => quote!(#default),
            Self::Choice { default: None } | Self::Flatten => {
                quote!(<#ty as ::std::default::Default>::default())
            }
            Self::Path { .. } => quote!(None),
        }
    }

    fn kind(&self, ty: &syn::Type) -> syn::Result<TokenStream> {
        Ok(match self {
            Self::Number { default, range } => {
                let (min, max) = (&range.start, &range.end);
                if is(ty, "f32") || is(ty, "f64") {
                    quote!(::drip::param::ParamKind::Float {
                        min: (#min) as f64, max: (#max) as f64, default: (#default) as f64,
                    })
                } else if is(ty, "i64") {
                    quote!(::drip::param::ParamKind::Int { min: #min, max: #max, default: #default })
                } else {
                    return Err(syn::Error::new(
                        ty.span(),
                        "numeric parameters are f32, f64 or i64",
                    ));
                }
            }
            Self::Bool { default } => quote!(::drip::param::ParamKind::Bool { default: #default }),
            Self::Choice { default: Some(default) } => quote!((#default).schema()),
            Self::Choice { default: None } => quote!(<#ty>::DEFAULT_SCHEMA),
            Self::Path { output, extensions } => {
                quote!(::drip::param::ParamKind::Path { output: #output, extensions: &#extensions })
            }
            Self::Flatten => unreachable!("flattened fields contribute their own specs"),
        })
    }
}

fn is(ty: &syn::Type, name: &str) -> bool {
    matches!(ty, syn::Type::Path(path) if path.path.is_ident(name))
}

pub(crate) fn expand(item: syn::DeriveInput) -> syn::Result<TokenStream> {
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
    let custom = item
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("validate"))
        .map(|attr| attr.parse_args::<syn::Path>())
        .transpose()?
        .map(|path| quote!(#path(self)?;));
    let mut groups = Vec::new();
    let mut defaults = Vec::new();
    let mut nested = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        let attr = field.attrs.iter().find(|a| a.path().is_ident("param")).ok_or_else(|| {
            syn::Error::new_spanned(field, "missing #[param(...)] or #[param(flatten)]")
        })?;
        let declared = Field::parse(attr, ty)?;
        let default = declared.default(ty);
        defaults.push(quote!(#ident: #default));
        if let Field::Flatten = declared {
            groups.push(quote!(<#ty as ::drip::param::Parameters>::SPECS));
            nested.push(quote!(::drip::param::Parameters::validate(&self.#ident)?;));
            continue;
        }
        let kind = declared.kind(ty)?;
        let key = ident.to_string();
        let label = documentation::label(&field.attrs, &key)?;
        let external = field.attrs.iter().any(|attr| attr.path().is_ident("external"));
        let docs = documentation::documentation(&field.attrs);
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
    Ok(quote! {
        impl ::drip::param::Parameters for #name {
            const SPECS: &'static [::drip::param::ParamSpec] =
                &::drip::param::concat_specs::<{0 #(+ (#groups).len())*}>(&[#(#groups),*]);
            fn validate(&self) -> ::drip::Result<()> {
                #(#nested)*
                #custom
                Ok(())
            }
        }
        impl ::std::default::Default for #name {
            fn default() -> Self {
                Self { #(#defaults),* }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_defaults_drive_default_and_schema() {
        let text = expand(syn::parse_quote! {
            struct P {
                #[param(default = 1.5, range = 0.5..=4.0)]
                contrast: f32,
                #[param(default = -12, range = -24..=-1)]
                min_ev: i64,
                #[param(default = true)]
                on: bool,
                #[param(default = Scale::Log)]
                scale: Scale,
                #[param]
                mode: Mode,
                #[external]
                #[param(open = ["icc", "icm"])]
                file: Option<PathBuf>,
                #[param(flatten)]
                range: Range,
            }
        })
        .unwrap()
        .to_string();
        for expected in [
            "contrast : 1.5",
            "min_ev : - 12",
            "on : true",
            "scale : Scale :: Log",
            "mode : < Mode as :: std :: default :: Default > :: default ()",
            "file : None",
            "range : < Range as :: std :: default :: Default > :: default ()",
            "ParamKind :: Float { min : (0.5) as f64 , max : (4.0) as f64 , default : (1.5) as f64 , }",
            "ParamKind :: Int { min : - 24 , max : - 1 , default : - 12 }",
            "(Scale :: Log) . schema ()",
            "< Mode > :: DEFAULT_SCHEMA",
            "ParamKind :: Path { output : false , extensions : & [\"icc\" , \"icm\"] }",
            ":: drip :: param :: Parameters :: validate (& self . range) ?",
            "external : true",
        ] {
            assert!(text.contains(expected), "missing `{expected}` in {text}");
        }
    }

    #[test]
    fn rejects_incomplete_or_conflicting_declarations() {
        for (field, message) in [
            (quote!(#[param(range = 0..=1)] x: f32), "expected default and range"),
            (quote!(#[param(default = 1, range = 0..=2)] x: u8), "numeric parameters are f32"),
            (quote!(#[param(default = 1, default = 2)] x: bool), "duplicate parameter key"),
            (quote!(#[param(open = "icc")] x: Option<PathBuf>), "expected [\"ext\", ...]"),
            (quote!(#[param(step = 1)] x: f32), "unknown parameter key"),
            (quote!(x: f32), "missing #[param"),
        ] {
            let item: syn::DeriveInput = syn::parse_quote!(struct P { #field });
            let error = expand(item).unwrap_err().to_string();
            assert!(error.contains(message), "{error}");
        }
    }
}
