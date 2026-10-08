//! Mechanical node/parameter adapters; data contracts remain on payloads and kernels.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

/// Include a locally defined node in the executable's generated catalogue.
/// Function Rustdoc supplies inspector help; optional `references = [(label, url), ...]`
/// supplies links. Keep implementation rationale in ordinary comments inside the function.
/// Named immutable arguments declare ports. `#[params] p: Parameters` supplies a
/// parameter record, or individual `#[param(kind)]` arguments generate one.
/// `#[context] ctx: &EvalContext<'_>` accesses evaluation resources and scale.
/// Functions return an `Arc<T>` or output tuple; a signature ending in `;`
/// declares a consumer without a kernel. Semantic predicates stay in the body.
/// Input syntax is `&T`, `Either<&A, &B>`, or `Option` of either;
/// qualified paths are accepted, but aliases for these wrappers are not resolved.
/// The function `foo_bar` declares `FooBarNode` and the typed handle named by `kind`.
#[proc_macro_attribute]
pub fn node(args: TokenStream, input: TokenStream) -> TokenStream {
    if let Ok(function) = syn::parse::<syn::ItemFn>(input.clone()) {
        return match node_function(args.into(), function, false) {
            Ok(tokens) => tokens.into(),
            Err(e) => e.into_compile_error().into(),
        };
    }
    if let Ok(declaration) = syn::parse::<syn::ForeignItemFn>(input.clone()) {
        let function = syn::ItemFn {
            attrs: declaration.attrs,
            vis: declaration.vis,
            sig: declaration.sig,
            block: Box::new(syn::parse_quote!({})),
        };
        return match node_function(args.into(), function, true) {
            Ok(tokens) => tokens.into(),
            Err(e) => e.into_compile_error().into(),
        };
    }
    syn::Error::new_spanned(
        proc_macro2::TokenStream::from(input),
        "node requires a function body or a function signature ending in ;",
    )
    .into_compile_error()
    .into()
}

/// Discover a concrete GUI implementation beside its `GuiNode` impl.
/// Runtime adapters belong to drip-gui; this emits only the local binding entry.
#[proc_macro_attribute]
pub fn gui_node(args: TokenStream, input: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new(proc_macro2::Span::call_site(), "gui_node takes no options")
            .into_compile_error()
            .into();
    }
    let item = parse_macro_input!(input as syn::ItemImpl);
    let syn::Type::Path(path) = &*item.self_ty else {
        return syn::Error::new_spanned(&item.self_ty, "expected a concrete GUI type")
            .into_compile_error()
            .into();
    };
    if !item.generics.params.is_empty() {
        return syn::Error::new_spanned(&item.generics, "GUI discovery requires a concrete type")
            .into_compile_error()
            .into();
    }
    let ty = &item.self_ty;
    let name = &path.path.segments.last().expect("type path").ident;
    let entry = format_ident!("__DRIP_GUI_{}", name.to_string().to_uppercase());
    quote! {
        #item
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static #entry: crate::node_ui::Binding = crate::node_ui::Binding::new::<#ty>();
    }
    .into()
}

/// Typed parameter fields are the source for persisted keys, defaults, editor
/// ranges and documentation. Flattening reuses a shared schema without nesting
/// existing project-file keys.
#[proc_macro_derive(Parameters, attributes(param, external))]
pub fn parameters(input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as syn::DeriveInput);
    match parameters_impl(item) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.into_compile_error().into(),
    }
}

fn parameters_impl(item: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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
    let mut reads = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        let attr = field.attrs.iter().find(|a| a.path().is_ident("param")).ok_or_else(|| {
            syn::Error::new_spanned(field, "missing #[param(kind)] or #[param(flatten)]")
        })?;
        let kind: syn::Expr = attr.parse_args()?;
        if matches!(&kind, syn::Expr::Path(p) if p.path.is_ident("flatten")) {
            groups.push(quote!(<#ty as ::drip::param::Parameters>::SPECS));
            reads.push(quote!(#ident: <#ty as ::drip::param::Parameters>::read(params)));
        } else {
            let key = ident.to_string();
            let external = field.attrs.iter().any(|a| a.path().is_ident("external"));
            let docs = documentation(&field.attrs);
            groups.push(quote!(&[{
                assert!(<#ty as ::drip::param::ParameterField>::TYPE.accepts(&(#kind)), "parameter field type disagrees with schema");
                ::drip::param::ParamSpec { name: #key, kind: #kind, external: #external, documentation: #docs }
            }]));
            reads.push(quote!(#ident: params.get::<#ty>(#key)));
        }
    }
    Ok(quote! {
        impl ::drip::param::Parameters for #name {
            const SPECS: &'static [::drip::param::ParamSpec] = &::drip::param::concat_specs::<{0 #(+ (#groups).len())*}>(&[#(#groups),*]);
            fn read(params: ::drip::param::Params<'_>) -> Self { Self { #(#reads),* } }
        }
    })
}

/// A finite parameter choice. Each unit variant declares its persisted spelling
/// with `#[choice("name")]`. Generates JSON conversion and `variant.schema()`;
/// use that schema in `#[param(...)]` on a field of this enum type.
#[proc_macro_derive(Choice, attributes(choice))]
pub fn choice(input: TokenStream) -> TokenStream {
    match choice_impl(parse_macro_input!(input as syn::DeriveInput)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn choice_impl(item: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
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
                ::drip::param::ParamKind::Choice { options: Self::OPTIONS, default: self.as_str() }
            }
        }
        impl ::drip::param::ParameterField for #name {
            const TYPE: ::drip::param::FieldType = ::drip::param::FieldType::Choice(Self::OPTIONS);
        }
        impl<'de> ::drip::__private::serde::Deserialize<'de> for #name {
            fn deserialize<D: ::drip::__private::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <String as ::drip::__private::serde::Deserialize>::deserialize(deserializer)?;
                match value.as_str() {
                    #(#names => Ok(Self::#variants),)*
                    _ => Err(::drip::__private::serde::de::Error::unknown_variant(&value, Self::OPTIONS)),
                }
            }
        }
        impl ::drip::__private::serde::Serialize for #name {
            fn serialize<S: ::drip::__private::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    })
}

fn documentation(attrs: &[syn::Attribute]) -> String {
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

fn input_type(ty: &syn::Type) -> syn::Result<proc_macro2::TokenStream> {
    match ty {
        syn::Type::Reference(r) if r.mutability.is_none() => {
            let t = &r.elem;
            Ok(quote!(::drip::ports::Read<#t>))
        }
        syn::Type::Path(path) => {
            let segment = path.path.segments.last().unwrap();
            if segment.ident == "Option" {
                let args = type_arguments(ty, "Option")?;
                let [inner] = args.as_slice() else {
                    return Err(syn::Error::new_spanned(ty, "expected Option<T>"));
                };
                let inner = input_type(inner)?;
                Ok(quote!(::drip::ports::Optional<#inner>))
            } else if segment.ident == "Either" {
                let args = type_arguments(ty, "Either")?;
                let [syn::Type::Reference(a), syn::Type::Reference(b)] = args.as_slice() else {
                    return Err(syn::Error::new_spanned(ty, "expected Either<&A, &B>"));
                };
                if a.mutability.is_some() || b.mutability.is_some() {
                    return Err(syn::Error::new_spanned(ty, "inputs must be immutable"));
                }
                let (a, b) = (&a.elem, &b.elem);
                Ok(quote!(::drip::ports::ReadEither<#a, #b>))
            } else {
                Err(syn::Error::new_spanned(
                    ty,
                    "use &T, Either<&A, &B>, or Option of either; wrapper aliases are not resolved",
                ))
            }
        }
        _ => Err(syn::Error::new_spanned(ty, "inputs must be immutable references")),
    }
}

fn type_arguments<'a>(ty: &'a syn::Type, wrapper: &str) -> syn::Result<Vec<&'a syn::Type>> {
    if let syn::Type::Path(p) = ty {
        let s = p.path.segments.last().unwrap();
        if s.ident == wrapper
            && let syn::PathArguments::AngleBracketed(a) = &s.arguments
        {
            return a
                .args
                .iter()
                .map(|argument| match argument {
                    syn::GenericArgument::Type(ty) => Ok(ty),
                    _ => Err(syn::Error::new_spanned(argument, "expected a type argument")),
                })
                .collect();
        }
    }
    Err(syn::Error::new_spanned(ty, format!("expected {wrapper}<...>")))
}

fn node_function(
    args: proc_macro2::TokenStream,
    mut function: syn::ItemFn,
    declaration: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    use syn::parse::Parser;
    let meta = syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated
        .parse2(args)?;
    let mut options: std::collections::BTreeMap<String, syn::Expr> =
        std::collections::BTreeMap::new();
    for m in meta {
        let key = m
            .path
            .get_ident()
            .ok_or_else(|| syn::Error::new_spanned(&m.path, "expected option name"))?
            .to_string();
        if !["kind", "id", "category", "name", "outputs", "actions", "references"]
            .contains(&key.as_str())
        {
            return Err(syn::Error::new_spanned(m, "unknown node option"));
        }
        if options.insert(key, m.value.clone()).is_some() {
            return Err(syn::Error::new_spanned(m, "duplicate node option"));
        }
    }
    let get = |key: &str| {
        options
            .get(key)
            .ok_or_else(|| syn::Error::new_spanned(&function.sig, format!("missing {key}")))
    };
    let kind = get("kind")?;
    let id = get("id")?;
    let category = get("category")?;
    let label = get("name")?;
    let outputs = get("outputs")?;
    if !function.sig.generics.params.is_empty()
        || function.sig.asyncness.is_some()
        || function.sig.unsafety.is_some()
        || function.sig.constness.is_some()
        || function.sig.abi.is_some()
    {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "node functions must be ordinary concrete functions",
        ));
    }
    let signature = node_arguments(&mut function)?;
    let params_type = &signature.params_type;
    let names = &signature.names;
    let inputs = &signature.inputs;
    let parameter_definition = &signature.parameter_definition;
    let bindings = &signature.bindings;
    let calls = &signature.calls;
    let syn::ReturnType::Type(_, return_type) = &function.sig.output else {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "expected Result<Outputs, KernelError>",
        ));
    };
    let result = type_arguments(return_type, "Result")?;
    let [output_types, _error] = result.as_slice() else {
        return Err(syn::Error::new_spanned(return_type, "expected Result<Outputs, KernelError>"));
    };
    let name = &function.sig.ident;
    let identity = name
        .to_string()
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().unwrap().to_uppercase().collect::<String>() + chars.as_str()
        })
        .collect::<String>();
    let kernel = format_ident!("{}Node", identity);
    let entry = format_ident!("__DRIP_ENTRY_{}", name.to_string().to_uppercase());
    let docs = documentation(&function.attrs);
    let references = options.get("references").cloned().unwrap_or_else(|| syn::parse_quote!([]));
    let actions = callbacks(options.get("actions"))?.into_iter().map(
        |(label, callback)| quote!(::drip::node::TypedAction { name: #label, run: #callback }),
    );
    let implementation = if declaration {
        // Check the signature through the adapter without emitting a callable
        // panic-only function for a declaration that has no implementation.
        let arguments = function.sig.inputs.iter().map(|arg| match arg {
            syn::FnArg::Typed(arg) => &arg.ty,
            _ => unreachable!(),
        });
        let ignored = function.sig.inputs.iter().map(|_| quote!(_));
        quote! {
            const _: ::drip::node::Kernel<#kernel> =
                |__params, (#(#bindings,)*), __context| {
                    let declaration: fn(#(#arguments),*) -> #return_type =
                        |#(#ignored),*| unreachable!("signature check only");
                    declaration(#(#calls),*)
                };
        }
    } else {
        quote!(#function)
    };
    let callback = if declaration {
        quote!(None)
    } else {
        quote!(Some(|__params, (#(#bindings,)*), __context| #name(#(#calls),*)))
    };
    Ok(quote! {
        #parameter_definition
        #implementation
        #[allow(non_camel_case_types)]
        pub struct #kernel;
        impl ::drip::node::NodeDeclaration for #kernel {
            type Parameters = #params_type;
            type Inputs = (#(#inputs,)*);
            type Outputs = #output_types;
            const ACTIONS: &'static [::drip::node::TypedAction<Self>] = &[#(#actions),*];
            const KERNEL: Option<::drip::node::Kernel<Self>> = #callback;
        }
        pub static #kind: ::drip::node::TypedNode<#kernel> = ::drip::node::TypedNode::new(#id, #category, #label, &[#(#names),*], &#outputs).documented(#docs).references(&#references);
        #[::drip::__private::linkme::distributed_slice(::drip::node::NODE_KINDS)]
        #[linkme(crate = ::drip::__private::linkme)]
        static #entry: &'static ::drip::node::NodeKind = #kind.kind();
    })
}

struct NodeArguments {
    params_type: proc_macro2::TokenStream,
    parameter_definition: proc_macro2::TokenStream,
    names: Vec<String>,
    inputs: Vec<proc_macro2::TokenStream>,
    bindings: Vec<syn::Ident>,
    calls: Vec<proc_macro2::TokenStream>,
}

fn node_arguments(function: &mut syn::ItemFn) -> syn::Result<NodeArguments> {
    let identity = function
        .sig
        .ident
        .to_string()
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            c.next().unwrap().to_uppercase().collect::<String>() + c.as_str()
        })
        .collect::<String>();
    let generated_params = format_ident!("{}Parameters", identity);
    let mut result = NodeArguments {
        params_type: quote!(()),
        parameter_definition: quote!(),
        names: Vec::new(),
        inputs: Vec::new(),
        bindings: Vec::new(),
        calls: Vec::new(),
    };
    let mut fields = Vec::new();
    let mut has_params = false;
    let mut has_context = false;
    for argument in &mut function.sig.inputs {
        let syn::FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(argument, "node functions have no receiver"));
        };
        let syn::Pat::Ident(pattern) = &*argument.pat else {
            return Err(syn::Error::new_spanned(&argument.pat, "node arguments must be named"));
        };
        let name = &pattern.ident;
        let ty = &argument.ty;
        let roles: Vec<_> = argument
            .attrs
            .iter()
            .filter(|a| {
                a.path().is_ident("param")
                    || a.path().is_ident("params")
                    || a.path().is_ident("context")
            })
            .collect();
        if roles.len() > 1 {
            return Err(syn::Error::new_spanned(argument, "an argument has exactly one role"));
        }
        match roles.first() {
            Some(attr) if attr.path().is_ident("params") => {
                if has_params || !fields.is_empty() {
                    return Err(syn::Error::new_spanned(
                        argument,
                        "use one #[params] record or inline #[param] fields",
                    ));
                }
                has_params = true;
                result.params_type = quote!(#ty);
                result.calls.push(quote!(__params));
            }
            Some(attr) if attr.path().is_ident("context") => {
                if has_context {
                    return Err(syn::Error::new_spanned(
                        argument,
                        "only one #[context] argument is allowed",
                    ));
                }
                has_context = true;
                result.calls.push(quote!(__context));
            }
            Some(_) => {
                if has_params {
                    return Err(syn::Error::new_spanned(
                        argument,
                        "use one #[params] record or inline #[param] fields",
                    ));
                }
                let attrs = &argument.attrs;
                fields.push(quote!(#(#attrs)* pub #name: #ty));
                result.calls.push(quote!(__params.#name));
            }
            None => {
                let binding = format_ident!("__input_{}", result.names.len());
                result.names.push(name.to_string());
                result.inputs.push(input_type(ty)?);
                result.calls.push(quote!(#binding));
                result.bindings.push(binding);
            }
        }
        argument.attrs.retain(|a| {
            !a.path().is_ident("param")
                && !a.path().is_ident("params")
                && !a.path().is_ident("context")
                && !a.path().is_ident("external")
                && !a.path().is_ident("doc")
        });
    }
    if !fields.is_empty() {
        result.params_type = quote!(#generated_params);
        let item: syn::DeriveInput =
            syn::parse2(quote!(pub struct #generated_params { #(#fields,)* }))?;
        let implementation = parameters_impl(item.clone())?;
        let mut emitted = item;
        if let syn::Data::Struct(data) = &mut emitted.data {
            for field in &mut data.fields {
                field
                    .attrs
                    .retain(|a| !a.path().is_ident("param") && !a.path().is_ident("external"));
            }
        }
        result.parameter_definition = quote!(#emitted #implementation);
    }
    Ok(result)
}

fn callbacks(expr: Option<&syn::Expr>) -> syn::Result<Vec<(&syn::Expr, &syn::Expr)>> {
    let Some(expr) = expr else { return Ok(Vec::new()) };
    let syn::Expr::Array(array) = expr else {
        return Err(syn::Error::new_spanned(expr, "expected [(name, function), ...]"));
    };
    array
        .elems
        .iter()
        .map(|element| {
            let syn::Expr::Tuple(pair) = element else {
                return Err(syn::Error::new_spanned(element, "expected (name, function)"));
            };
            if pair.elems.len() != 2 {
                return Err(syn::Error::new_spanned(element, "expected (name, function)"));
            }
            Ok((&pair.elems[0], &pair.elems[1]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{choice_impl, input_type};

    #[test]
    fn input_syntax_accepts_qualified_wrappers_and_rejects_aliases() {
        for ty in [
            "&ImageAlias",
            "drip::ports::Either<&A, &B>",
            "std::option::Option<drip::ports::Either<&A, &B>>",
        ] {
            assert!(input_type(&syn::parse_str(ty).unwrap()).is_ok(), "{ty}");
        }
        for ty in ["ImageInput", "ImageInput<'_>", "Option<ImageInput<'_>>"] {
            let error = input_type(&syn::parse_str(ty).unwrap()).unwrap_err();
            assert!(error.to_string().contains("wrapper aliases are not resolved"), "{error}");
        }
    }

    #[test]
    fn named_arguments_keep_ports_parameters_and_context_distinct() {
        let mut function = syn::parse_quote! {
            fn scale(
                image: &Image<Gpu>,
                #[param(ParamKind::Float { default: 1.0, min: 0.0, max: 4.0 })]
                gain: f64,
                mask: Option<&Mask<Cpu>>,
                #[context] context: &EvalContext<'_>,
            ) -> Result<Arc<Image<Gpu>>, KernelError> { todo!() }
        };
        let arguments = super::node_arguments(&mut function).unwrap();
        assert_eq!(arguments.names, ["image", "mask"]);
        assert_eq!(arguments.params_type.to_string(), "ScaleParameters");
        assert_eq!(arguments.calls.len(), 4);
        assert!(
            function
                .sig
                .inputs
                .iter()
                .all(|arg| { matches!(arg, syn::FnArg::Typed(arg) if arg.attrs.is_empty()) })
        );
    }

    #[test]
    fn complete_named_node_expands_to_rust_items() {
        let options = quote::quote!(
            kind = SCALE,
            id = "test.scale",
            category = "test",
            name = "Scale",
            outputs = ["image"]
        );
        let function = syn::parse_quote! {
            /// Scale values in their declared additive coordinates.
            fn scale(image: &Image<Gpu>, #[param(kind)] gain: f32)
                -> Result<Arc<Image<Gpu>>, KernelError> { todo!() }
        };
        let expanded = super::node_function(options, function, false).unwrap();
        syn::parse2::<syn::File>(expanded).unwrap();
    }

    #[test]
    fn roles_cannot_duplicate_or_mix_parameter_record_with_fields() {
        for definition in [
            "fn bad(#[params] p: P, #[param(kind)] gain: f32) {}",
            "fn bad(#[context] a: &C, #[context] b: &C) {}",
            "fn bad(#[params] #[context] p: P) {}",
            "fn bad((a, b): (&A, &B)) {}",
        ] {
            let mut function = syn::parse_str(definition).unwrap();
            assert!(super::node_arguments(&mut function).is_err(), "{definition}");
        }
    }

    #[test]
    fn choices_reject_ambiguous_or_nonfinite_declarations() {
        for (declaration, message) in [
            (
                "enum Bad { #[choice(\"same\")] A, #[choice(\"same\")] B }",
                "duplicate persisted choice name",
            ),
            ("enum Bad { #[choice(\"a\")] A(u32) }", "choice variants must be unit variants"),
            ("enum Bad {}", "choices must be concrete and nonempty"),
            ("enum Bad { A }", "expected one #[choice(\"name\")]"),
        ] {
            let error = choice_impl(syn::parse_str(declaration).unwrap()).unwrap_err();
            assert_eq!(error.to_string(), message);
        }
    }
}
