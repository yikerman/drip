use proc_macro2::TokenStream;
use quote::quote;

use super::{
    expand,
    parse::{Direction, Signature},
};

#[test]
fn rejects_invalid_options() {
    for (options, message) in [
        (quote!(contract = c), "missing id"),
        (quote!(id = "x", contract = c, checks = c), "unknown node option"),
        (quote!(id = "x", contract = c, parameters = Params::SPECS), "unknown node option"),
        (quote!(id = "x", id = "y", contract = c), "duplicate node option"),
        (quote!(id = "x", contract = c, name = "X", name = "Y"), "duplicate node option"),
        (
            quote!(id = "x", contract = c, category = "raw", category = "color"),
            "duplicate node option",
        ),
        (quote!(id = "x", contract = c, references = [], references = []), "duplicate node option"),
        (quote!(node::id = "x", contract = c), "expected option name"),
    ] {
        let function = syn::parse_quote! {
            fn x(c: &Context, p: &Params) -> Result<()> { Ok(()) }
        };
        let error = expand(options, function).unwrap_err();
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn nodes_with_outputs_need_a_contract() {
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params, output: Write<Cpu<Image>>) -> Result<()> { Ok(()) }
    };
    let error = expand(quote!(id = "x"), function).unwrap_err();
    assert_eq!(error.to_string(), "a node with outputs needs a contract");
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params, image: Read<Cpu<Image>>) -> Result<()> { Ok(()) }
    };
    generated_module(expand(quote!(id = "x"), function).unwrap());
}

#[test]
fn an_unread_port_drops_its_underscore() {
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params, _image: Read<Cpu<Image>>) -> Result<()> { Ok(()) }
    };
    let module = generated_module(expand(quote!(id = "x"), function).unwrap());
    assert_port_specs(
        &module,
        "inputs",
        quote!(::drip::ports::PortSpec::of::<Cpu<Image>, ::drip::ports::Any>("image")),
    );
}

#[test]
fn sources_load_cpu_outputs_at_admission_without_a_contract() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn read(p: &Settings, mosaic: Load<'_, Bayer>, metadata: Load<'_, Facts>) -> Result<()> {
            Ok(())
        }
    };
    let signature = Signature::parse(&function.sig).unwrap();
    assert!(signature.source);
    let module = generated_module(expand(quote!(id = "raw"), function.clone()).unwrap());
    assert_port_specs(
        &module,
        "outputs",
        quote!(
            ::drip::ports::PortSpec::of::<::drip::ports::Cpu<Bayer>, ::drip::ports::Any>("mosaic"),
            ::drip::ports::PortSpec::of::<::drip::ports::Cpu<Facts>, ::drip::ports::Any>(
                "metadata"
            )
        ),
    );
    let items = &module.content.as_ref().unwrap().1;
    assert!(items.iter().any(|item| matches!(item, syn::Item::Fn(f) if f.sig.ident == "load")));
    let error = expand(quote!(id = "raw", contract = c), function).unwrap_err();
    assert!(error.to_string().contains("remove the contract"));
    for declaration in [
        quote!(
            fn x(p: &P, a: Load<A>, b: Read<Cpu<B>>) {}
        ),
        quote!(
            fn x(c: &C, p: &P, a: Load<A>) {}
        ),
    ] {
        let error = expand(quote!(id = "x"), syn::parse2(declaration).unwrap()).unwrap_err();
        assert!(error.to_string().contains("only Load ports"), "{error}");
    }
}

#[test]
fn actions_are_declared_with_the_node_and_named_from_their_function() {
    let function = syn::parse_quote! {
        fn tiff(c: &Context, p: &Export, image: Read<Cpu<Rgb>>) -> Result<()> { Ok(()) }
    };
    let module = generated_module(expand(quote!(id = "x", action = write_file), function).unwrap());
    let action = kind_field(&module, "action");
    let action = quote!(#action).to_string();
    assert!(action.starts_with("Some (:: drip :: definition :: Action { name : \"Write file\""));
    assert!(action.contains(
        "super :: write_file (& node . parameters , inputs . bind :: < Definition > () ?"
    ));
    let function = syn::parse_quote! { fn x(c: &Context, p: &P) -> Result<()> { Ok(()) } };
    let module = generated_module(expand(quote!(id = "x"), function).unwrap());
    let action = kind_field(&module, "action");
    assert_eq!(quote!(#action).to_string(), "None");
}

#[test]
fn assets_load_at_admission_and_reach_the_action() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn tiff(c: &Context, p: &Export, image: Read<Cpu<Rgb>>) -> Result<()> { Ok(()) }
    };
    let options = quote!(id = "x", action = export, assets);
    let module = generated_module(expand(options, function.clone()).unwrap());
    let field = kind_field(&module, "loads_assets");
    assert_eq!(quote!(#field).to_string(), "true");
    let action = kind_field(&module, "action");
    assert!(quote!(#action).to_string().contains(
        "super :: export (& node . parameters , node . assets () ? , inputs . bind :: < Definition > () ?"
    ));
    let items = &module.content.as_ref().unwrap().1;
    let admit = items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == "definition" => Some(quote!(#f).to_string()),
            _ => None,
        })
        .unwrap();
    assert!(admit.contains("assets : :: drip :: definition :: Assets :: load (& parameters)"));
    for options in [quote!(id = "x", assets, assets), quote!(id = "x", assets = true)] {
        let error = expand(options, function.clone()).unwrap_err();
        assert_eq!(error.to_string(), "expected a single `assets` flag");
    }
    let source =
        syn::parse_quote! { fn read(p: &Settings, raw: Load<'_, Bayer>) -> Result<()> { Ok(()) } };
    let error = expand(quote!(id = "x", assets), source).unwrap_err();
    assert!(error.to_string().contains("remove assets"), "{error}");
}

#[test]
fn rejects_unsupported_signatures() {
    let declarations = [
        (
            quote!(
                async fn x(c: &C, p: &P) {}
            ),
            "node must be a concrete synchronous safe function",
        ),
        (
            quote!(
                unsafe fn x(c: &C, p: &P) {}
            ),
            "node must be a concrete synchronous safe function",
        ),
        (
            quote!(
                fn x<T>(c: &C, p: &P) {}
            ),
            "node must be a concrete synchronous safe function",
        ),
        (
            quote!(
                fn x(c: &C) {}
            ),
            "expected context and &parameters",
        ),
        (
            quote!(
                fn x(c: &C, p: P) {}
            ),
            "parameters must be borrowed",
        ),
        (
            quote!(
                fn x(c: &C, p: &mut P) {}
            ),
            "parameters must be immutable",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, (a, b): Read<A>) {}
            ),
            "ports must be named",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, node: Read<A>) {}
            ),
            "node is reserved for the generated node handle",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: &A) {}
            ),
            "expected Read<Port> or Write<Port>",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Read) {}
            ),
            "expected a concrete port type",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Read<'_>) {}
            ),
            "expected <Port> or <Port, Requirement>",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Read<A, B, C>) {}
            ),
            "expected <Port> or <Port, Requirement>",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Either<A, B>) {}
            ),
            "expected Read, Write or Load; wrapper aliases are not resolved",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Alias<A>) {}
            ),
            "expected Read, Write or Load; wrapper aliases are not resolved",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Option<Write<A>>) {}
            ),
            "output Write ports cannot be optional",
        ),
    ];
    for (declaration, message) in declarations {
        let function = syn::parse2(declaration).unwrap();
        let error = expand(quote!(id = "x", contract = contract), function).unwrap_err();
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn accepts_qualified_ports_and_indexes_each_direction_independently() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn mix(
            context: &Context,
            parameters: &Params,
            first: ports::Write<'_, Cpu<A>>,
            input: ports::Read<'_, Cpu<B>>,
            second: ports::Write<Cpu<C>>,
            other: ports::Read<Device<D>>,
        ) -> Result<()> { Ok(()) }
    };
    let signature = Signature::parse(&function.sig).unwrap();
    let ports: Vec<_> = signature
        .ports
        .iter()
        .map(|port| (port.name.to_string(), port.direction == Direction::Output, port.index))
        .collect();
    assert_eq!(
        ports,
        [
            ("first".into(), true, 0),
            ("input".into(), false, 0),
            ("second".into(), true, 1),
            ("other".into(), false, 1),
        ]
    );

    let expanded = expand(quote!(id = "mix", contract = describe), function).unwrap();
    let module = generated_module(expanded);
    let run = generated_method(&module, "run");
    let kernel = kernel_call(run, "mix");
    let arguments: Vec<_> =
        kernel.args.iter().skip(2).map(|arg| quote!(#arg).to_string()).collect();
    let write = |slot: TokenStream| quote!(#slot.write::<::drip::ports::Any>()).to_string();
    assert_eq!(arguments[0], write(quote!(output_0)));
    assert_eq!(arguments[1], quote!(inputs.input).to_string());
    assert_eq!(arguments[2], write(quote!(output_1)));
    assert_eq!(arguments[3], quote!(inputs.other).to_string());
    let inputs = generated_struct(&module, "Inputs");
    let fields: Vec<_> =
        inputs.fields.iter().map(|f| f.ident.as_ref().unwrap().to_string()).collect();
    assert_eq!(fields, ["input", "other", "_lifetime"]);
}

#[test]
fn preserves_public_bindings_and_metadata() {
    let function = syn::parse_quote! {
        /// First line.
        /// Second line.
        pub fn source(context: &Context, parameters: &Params, image: Write<Cpu<Image>>)
            -> Result<()> { Ok(()) }
    };
    let module = generated_module(
        expand(
            quote!(
                id = "source",
                contract = describe,
                references = [("paper", "https://example.org")],
            ),
            function,
        )
        .unwrap(),
    );
    assert_eq!(module.ident, "source");
    assert!(matches!(module.vis, syn::Visibility::Public(_)));
    let items = &module.content.as_ref().unwrap().1;
    for expected in ["Definition", "Ports", "Inputs"] {
        assert!(items.iter().any(|item| matches!(item, syn::Item::Struct(item)
            if item.ident == expected && matches!(item.vis, syn::Visibility::Public(_)))));
    }
    for expected in ["definition", "add"] {
        assert!(items.iter().any(|item| matches!(item, syn::Item::Fn(item)
            if item.sig.ident == expected && matches!(item.vis, syn::Visibility::Public(_)))));
    }
    for expected in ["REGISTRATION", "KIND"] {
        assert!(items.iter().any(|item| matches!(item, syn::Item::Static(item)
            if item.ident == expected)));
    }
    for method in ["kind", "parameters", "contract", "run", "bind"] {
        generated_method(&module, method);
    }
    assert_eq!(literal(&module, "help"), "First line.\nSecond line.");
}

#[test]
fn generates_valid_rust_for_zero_and_multiple_ports() {
    for declaration in [
        quote!(
            fn x(c: &C, p: &P) {}
        ),
        quote!(
            fn x(c: &C, p: &P, a: Read<A>, b: Read<B>, c: Write<C>, d: Write<D>) {}
        ),
    ] {
        let function = syn::parse2(declaration).unwrap();
        generated_module(expand(quote!(id = "x", contract = c), function).unwrap());
    }
}

#[test]
fn supplies_default_and_explicit_display_metadata() {
    for (options, expected_name, expected_category) in [
        (quote!(id = "raw", contract = c), "raw", "processing"),
        (
            quote!(id = "raw", name = "RAW input", category = "input", contract = c),
            "RAW input",
            "input",
        ),
    ] {
        let function = syn::parse_quote!(
            fn raw(context: &Context, parameters: &Params) {}
        );
        let module = generated_module(expand(options, function).unwrap());
        for (name, expected) in
            [("id", "raw"), ("name", expected_name), ("category", expected_category)]
        {
            assert_eq!(literal(&module, name), expected);
        }
    }
}

#[test]
fn optional_inputs_keep_their_port_type_and_direction_index() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn mix(
            context: &Context,
            parameters: &Params,
            image: Read<Cpu<Image>>,
            metadata: Option<ports::Read<'_, Cpu<Metadata>>>,
            output: Write<Cpu<Image>>,
            mask: std::option::Option<ports::Read<'_, Device<Mask>>>,
        ) {}
    };
    let signature = Signature::parse(&function.sig).unwrap();
    let inputs: Vec<_> = signature
        .inputs()
        .map(|port| {
            let ty = &port.ty;
            (port.name.to_string(), quote!(#ty).to_string(), port.optional, port.index)
        })
        .collect();
    assert_eq!(
        inputs,
        [
            ("image".into(), quote!(Cpu<Image>).to_string(), false, 0),
            ("metadata".into(), quote!(Cpu<Metadata>).to_string(), true, 1),
            ("mask".into(), quote!(Device<Mask>).to_string(), true, 2),
        ]
    );
    let module = generated_module(expand(quote!(id = "mix", contract = c), function).unwrap());
    let optional: Vec<_> = port_specs(&module, "inputs")
        .iter()
        .map(|spec| matches!(spec, syn::Expr::MethodCall(spec) if spec.method == "optional"))
        .collect();
    assert_eq!(optional, [false, true, true]);
}

#[test]
fn optional_execution_is_nullable_and_contracts_keep_one_description_per_input() {
    let function = syn::parse_quote! {
        fn x(context: &Context, parameters: &Params, image: Read<Cpu<Image>>, metadata: Option<Read<Cpu<Metadata>>>) {}
    };
    let module = generated_module(expand(quote!(id = "x", contract = c), function).unwrap());
    let run = generated_method(&module, "run");
    let call = kernel_call(run, "x");
    assert!(matches!(&call.args[0], syn::Expr::Path(context) if context.path.is_ident("context")));
    let arguments: Vec<_> = call.args.iter().skip(2).map(|a| quote!(#a).to_string()).collect();
    assert_eq!(arguments, [quote!(inputs.image).to_string(), quote!(inputs.metadata).to_string()]);
    // Binding rejects an absent required input and preserves an optional None.
    let bind = generated_method(&module, "bind");
    let bind = quote!(#bind).to_string();
    let (image, metadata) = bind.split_once("metadata :").unwrap();
    assert!(image.contains(":: bind") && image.contains("ok_or"), "{image}");
    assert!(metadata.contains(":: bind") && !metadata.contains("ok_or"), "{metadata}");

    let contract = generated_method(&module, "contract");
    assert_eq!(contract.sig.inputs.len(), 3);
    let syn::FnArg::Typed(global) = &contract.sig.inputs[1] else {
        panic!("expected the global evaluation configuration")
    };
    let ty = &global.ty;
    assert_eq!(quote!(#ty).to_string(), quote!(&::drip::runtime::GlobalContext).to_string());
    // Requirement checks bind each input before the contract call; only a
    // required input rejects an absent description.
    let checks: Vec<_> = contract.block.stmts[..2].iter().map(|s| quote!(#s).to_string()).collect();
    assert!(checks[0].contains(":: drip :: ports :: input") && checks[0].contains("ok_or"));
    assert!(checks[1].contains(":: drip :: ports :: input") && !checks[1].contains("ok_or"));
    let syn::Stmt::Local(described) = &contract.block.stmts[2] else {
        panic!("expected contract call")
    };
    let syn::Expr::Try(described) = described.init.as_ref().unwrap().expr.as_ref() else {
        panic!("expected fallible contract")
    };
    let syn::Expr::Call(described) = described.expr.as_ref() else {
        panic!("expected contract arguments")
    };
    let arguments: Vec<_> = described.args.iter().map(|a| quote!(#a).to_string()).collect();
    assert_eq!(arguments, ["global", "& self . parameters", "image", "metadata"]);
}

#[test]
fn port_specs_carry_requirements_and_default_to_any() {
    let function = syn::parse_quote! {
        fn x(
            context: &Context,
            parameters: &Params,
            image: Read<Cpu<Rgb>>,
            coefficients: Option<Read<ports::Device<Coefficients>>>,
            working: Read<'_, Device<Rgb>, Rec2020>,
            output: Write<Cpu<Rgb>, Rec2020>,
        ) {}
    };
    let module = generated_module(expand(quote!(id = "x", contract = c), function).unwrap());
    for (method, expected) in [
        (
            "inputs",
            quote!(
                ::drip::ports::PortSpec::of::<Cpu<Rgb>, ::drip::ports::Any>("image"),
                ::drip::ports::PortSpec::of::<ports::Device<Coefficients>, ::drip::ports::Any>(
                    "coefficients"
                )
                .optional(),
                ::drip::ports::PortSpec::of::<Device<Rgb>, Rec2020>("working")
            ),
        ),
        ("outputs", quote!(::drip::ports::PortSpec::of::<Cpu<Rgb>, Rec2020>("output"))),
    ] {
        assert_port_specs(&module, method, expected);
    }
}

/// A field of the generated static `KIND`.
fn kind_field<'a>(module: &'a syn::ItemMod, name: &str) -> &'a syn::Expr {
    let kind = module
        .content
        .as_ref()
        .unwrap()
        .1
        .iter()
        .find_map(|item| match item {
            syn::Item::Static(item) if item.ident == "KIND" => Some(item),
            _ => None,
        })
        .expect("generated KIND");
    let syn::Expr::Struct(value) = kind.expr.as_ref() else { panic!("expected a Kind value") };
    &value
        .fields
        .iter()
        .find(|field| matches!(&field.member, syn::Member::Named(member) if member == name))
        .unwrap_or_else(|| panic!("missing KIND field {name}"))
        .expr
}

fn literal(module: &syn::ItemMod, name: &str) -> String {
    let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(value), .. }) = kind_field(module, name)
    else {
        panic!("expected a literal {name}")
    };
    value.value()
}

/// The port specs in `KIND.inputs` or `KIND.outputs`.
fn port_specs(module: &syn::ItemMod, field: &str) -> Vec<syn::Expr> {
    let syn::Expr::Reference(specs) = kind_field(module, field) else { panic!("expected &[..]") };
    let syn::Expr::Array(specs) = specs.expr.as_ref() else { panic!("expected an array") };
    specs.elems.iter().cloned().collect()
}

fn assert_port_specs(module: &syn::ItemMod, field: &str, expected: TokenStream) {
    let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    let expected = syn::parse::Parser::parse2(parser, expected).unwrap();
    let render =
        |exprs: Vec<syn::Expr>| exprs.iter().map(|e| quote!(#e).to_string()).collect::<Vec<_>>();
    assert_eq!(render(port_specs(module, field)), render(expected.into_iter().collect()));
}

fn kernel_call<'a>(run: &'a syn::ImplItemFn, name: &str) -> &'a syn::ExprCall {
    run.block
        .stmts
        .iter()
        .find_map(|statement| {
            let syn::Stmt::Expr(syn::Expr::Try(expr), _) = statement else {
                return None;
            };
            let syn::Expr::Call(call) = expr.expr.as_ref() else {
                return None;
            };
            let syn::Expr::Path(path) = call.func.as_ref() else {
                return None;
            };
            (path.path.segments.last()?.ident == name).then_some(call)
        })
        .unwrap_or_else(|| panic!("missing kernel call {name}"))
}

fn generated_struct<'a>(module: &'a syn::ItemMod, name: &str) -> &'a syn::ItemStruct {
    module
        .content
        .as_ref()
        .unwrap()
        .1
        .iter()
        .find_map(|item| match item {
            syn::Item::Struct(item) if item.ident == name => Some(item),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing generated struct {name}"))
}

fn generated_module(tokens: TokenStream) -> syn::ItemMod {
    let file: syn::File = syn::parse2(tokens).unwrap();
    file.items
        .into_iter()
        .find_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        })
        .unwrap()
}

fn generated_method<'a>(module: &'a syn::ItemMod, name: &str) -> &'a syn::ImplItemFn {
    module
        .content
        .as_ref()
        .unwrap()
        .1
        .iter()
        .find_map(|item| {
            let syn::Item::Impl(implementation) = item else {
                return None;
            };
            implementation.items.iter().find_map(|item| {
                let syn::ImplItem::Fn(method) = item else {
                    return None;
                };
                (method.sig.ident == name).then_some(method)
            })
        })
        .unwrap_or_else(|| panic!("missing generated method {name}"))
}
