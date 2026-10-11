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
            "expected Read or Write; wrapper aliases are not resolved",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Alias<A>) {}
            ),
            "expected Read or Write; wrapper aliases are not resolved",
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
    assert!(arguments[1].contains("Read :: < Cpu < B > , :: drip :: ports :: Any > :: bind"));
    assert_eq!(arguments[2], write(quote!(output_1)));
    assert!(arguments[3].contains("Read :: < Device < D > , :: drip :: ports :: Any > :: bind"));
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
    for expected in ["Definition", "Ports"] {
        assert!(items.iter().any(|item| matches!(item, syn::Item::Struct(item)
            if item.ident == expected && matches!(item.vis, syn::Visibility::Public(_)))));
    }
    for expected in ["definition", "add"] {
        assert!(items.iter().any(|item| matches!(item, syn::Item::Fn(item)
            if item.sig.ident == expected && matches!(item.vis, syn::Visibility::Public(_)))));
    }
    assert!(items.iter().any(|item| matches!(item, syn::Item::Static(item)
        if item.ident == "REGISTRATION")));
    for method in ["metadata", "parameters", "inputs", "outputs", "contract", "run"] {
        generated_method(&module, method);
    }
    let metadata = generated_method(&module, "metadata");
    let syn::Stmt::Expr(syn::Expr::Struct(value), _) = &metadata.block.stmts[0] else {
        panic!("expected metadata value")
    };
    let help = value
        .fields
        .iter()
        .find(|field| matches!(&field.member, syn::Member::Named(name) if name == "help"))
        .unwrap();
    let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(help), .. }) = &help.expr else {
        panic!("expected Rustdoc help")
    };
    assert_eq!(help.value(), "First line.\nSecond line.");
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
        let metadata = generated_method(&module, "metadata");
        let syn::Stmt::Expr(syn::Expr::Struct(value), _) = &metadata.block.stmts[0] else {
            panic!("expected metadata value")
        };
        for (name, expected) in
            [("id", "raw"), ("name", expected_name), ("category", expected_category)]
        {
            let field = value
                .fields
                .iter()
                .find(|field| matches!(&field.member, syn::Member::Named(ident) if ident == name))
                .unwrap();
            let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(actual), .. }) = &field.expr
            else {
                panic!("expected literal display metadata")
            };
            assert_eq!(actual.value(), expected);
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
    let inputs = generated_method(&module, "inputs");
    let syn::Stmt::Expr(syn::Expr::Macro(inputs), _) = &inputs.block.stmts[0] else {
        panic!("expected input specs")
    };
    let specs = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    let specs = syn::parse::Parser::parse2(specs, inputs.mac.tokens.clone()).unwrap();
    let optional: Vec<_> = specs
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
    let required = &call.args[2];
    let optional = &call.args[3];
    let required = quote!(#required).to_string();
    let optional = quote!(#optional).to_string();
    assert!(required.contains(":: bind"));
    assert!(required.contains("ok_or_else"), "required input must reject absent data");
    assert!(optional.contains(":: bind"));
    assert!(!optional.contains("ok_or_else"), "optional input preserves None");

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
    assert!(checks[0].contains(":: drip :: ports :: input") && checks[0].contains("ok_or_else"));
    assert!(checks[1].contains(":: drip :: ports :: input") && !checks[1].contains("ok_or_else"));
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

fn assert_port_specs(module: &syn::ItemMod, method: &str, expected: TokenStream) {
    let method = generated_method(module, method);
    let syn::Stmt::Expr(syn::Expr::Macro(specs), _) = &method.block.stmts[0] else {
        panic!("expected generated port specs")
    };
    let render = |tokens| {
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let expressions = syn::parse::Parser::parse2(parser, tokens).unwrap();
        expressions.iter().map(|expr| quote!(#expr).to_string()).collect::<Vec<_>>()
    };
    assert_eq!(render(specs.mac.tokens.clone()), render(expected));
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
