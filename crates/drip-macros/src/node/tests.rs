use proc_macro2::TokenStream;
use quote::quote;

use super::{
    expand,
    parse::{Direction, Family, Signature},
};

#[test]
fn rejects_invalid_options() {
    for (options, message) in [
        (quote!(contract = c), "missing id"),
        (quote!(id = "x", contract = c, checks = c), "unknown node option"),
        (quote!(id = "x", contract = c, parameters = Params::SPECS), "unknown node option"),
        (quote!(id = "x", id = "y", contract = c), "duplicate node option"),
        (quote!(id = "x", contract = c, name = "X", name = "Y"), "duplicate node option"),
        (quote!(id = "x", contract = c, tags = [], tags = []), "duplicate node option"),
        (quote!(id = "x", contract = c, tags = tag::RAW), "expected tags = [tag, ...]"),
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
        fn x(c: &Context, p: &Params, output: Write<Image>) -> Result<()> { Ok(()) }
    };
    let error = expand(quote!(id = "x"), function).unwrap_err();
    assert_eq!(error.to_string(), "a node with outputs needs a contract");
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params, image: Read<Image>) -> Result<()> { Ok(()) }
    };
    generated_module(expand(quote!(id = "x"), function).unwrap());
}

#[test]
fn an_unread_port_drops_its_underscore() {
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params, _image: Read<Image>) -> Result<()> { Ok(()) }
    };
    let module = generated_module(expand(quote!(id = "x"), function).unwrap());
    assert_port_specs(
        &module,
        "inputs",
        quote!(::drip::ports::PortSpec::of::<Image, ::drip::ports::Any>("image")),
    );
}

#[test]
fn foreign_nodes_publish_host_outputs_without_a_contract() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn read(p: &Settings, mosaic: Publish<'_, Bayer>, metadata: Publish<'_, Facts>) -> Result<()> {
            Ok(())
        }
    };
    let signature = Signature::parse(&function.sig).unwrap();
    assert_eq!(signature.family, Family::Publish);
    let module =
        generated_module(expand(quote!(id = "raw", foreign = disk), function.clone()).unwrap());
    assert_port_specs(
        &module,
        "outputs",
        quote!(
            ::drip::ports::PortSpec::of::<Bayer, ::drip::ports::Any>("mosaic"),
            ::drip::ports::PortSpec::of::<Facts, ::drip::ports::Any>("metadata")
        ),
    );
    let shape = kind_field(&module, "shape");
    let codecs = quote!(::drip::shape::Codec::of::<Bayer>(), ::drip::shape::Codec::of::<Facts>());
    let expected = quote!(::drip::shape::Shape::Foreign(::drip::shape::Cache::Disk(&[#codecs])));
    assert_eq!(quote!(#shape).to_string(), expected.to_string());
    let contract = &generated_method(&module, "contract").block;
    assert!(quote!(#contract).to_string().ends_with("Ok (vec ! []) }"), "{}", quote!(#contract));
    let module =
        generated_module(expand(quote!(id = "raw", foreign = memory), function.clone()).unwrap());
    let shape = kind_field(&module, "shape");
    let expected = quote!(::drip::shape::Shape::Foreign(::drip::shape::Cache::Memory));
    assert_eq!(quote!(#shape).to_string(), expected.to_string());
    for (options, message) in [
        (quote!(id = "raw", foreign = disk, contract = c), "remove the contract"),
        (quote!(id = "raw"), "Publish ports declare foreign = memory or disk"),
        (quote!(id = "raw", foreign = cloud), "expected foreign = memory or disk"),
        (
            quote!(id = "raw", foreign = disk, warp),
            "at most one of map, warp, resample, reduce, bins, stencil, foreign and meta",
        ),
    ] {
        let error = expand(options, function.clone()).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    for declaration in [
        quote!(
            fn x(p: &P, a: Publish<A>, b: Write<B>) {}
        ),
        quote!(
            fn x(c: &C, p: &P, a: Publish<A>) {}
        ),
    ] {
        let function = syn::parse2(declaration).unwrap();
        let error = expand(quote!(id = "x", foreign = memory), function).unwrap_err();
        assert!(error.to_string().contains("Read inputs and Publish outputs"), "{error}");
    }
    let kernel = syn::parse_quote! { fn x(c: &C, p: &P, a: Read<A>) {} };
    let error = expand(quote!(id = "x", foreign = memory), kernel).unwrap_err();
    assert!(error.to_string().contains("through Publish ports"), "{error}");
}

#[test]
fn foreign_nodes_download_their_inputs_before_the_call() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn x(p: &P, image: Read<'_, Bayer>, flat: Option<Read<'_, Bayer>>, output: Publish<'_, Bayer>) -> Result<()> {
            Ok(())
        }
    };
    let module = generated_module(expand(quote!(id = "x", foreign = memory), function).unwrap());
    let run = generated_method(&module, "run");
    let body = quote!(#run).to_string();
    let call = body.find("super :: x").unwrap();
    for download in ["inputs . image . data . host () ?", "input . data . host () ?"] {
        assert!(body.find(download).is_some_and(|at| at < call), "{body}");
    }
    let call = kernel_call(run, "x");
    let arguments: Vec<_> = call.args.iter().map(|arg| quote!(#arg).to_string()).collect();
    assert_eq!(
        arguments[3],
        quote!(::drip::ports::adapter::publish::<Bayer>(&mut output_0)).to_string()
    );
}

#[test]
fn actions_are_declared_with_the_node_and_named_from_their_function() {
    let function = syn::parse_quote! {
        fn tiff(c: &Context, p: &Export, image: Read<Rgb>) -> Result<()> { Ok(()) }
    };
    let module = generated_module(expand(quote!(id = "x", action = write_file), function).unwrap());
    let action = kind_field(&module, "action");
    let action = quote!(#action).to_string();
    assert!(action.starts_with("Some (:: drip :: definition :: Action { name : \"Write file\""));
    assert!(action.contains("super :: write_file (& node . parameters , inputs)"));
    let function = syn::parse_quote! { fn x(c: &Context, p: &P) -> Result<()> { Ok(()) } };
    let module = generated_module(expand(quote!(id = "x"), function).unwrap());
    let action = kind_field(&module, "action");
    assert_eq!(quote!(#action).to_string(), "None");
}

#[test]
fn assets_load_when_first_used_and_reach_the_action() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn tiff(c: &Context, p: &Export, image: Read<Rgb>) -> Result<()> { Ok(()) }
    };
    let options = quote!(id = "x", action = export, assets);
    let module = generated_module(expand(options, function.clone()).unwrap());
    let action = kind_field(&module, "action");
    assert!(
        quote!(#action)
            .to_string()
            .contains("super :: export (& node . parameters , node . assets () ? , inputs)")
    );
    let items = &module.content.as_ref().unwrap().1;
    let admit = items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == "definition" => Some(quote!(#f).to_string()),
            _ => None,
        })
        .unwrap();
    assert!(admit.contains("assets : Default :: default ()"), "admission reads no files");
    for options in [quote!(id = "x", assets, assets), quote!(id = "x", assets = true)] {
        let error = expand(options, function.clone()).unwrap_err();
        assert_eq!(error.to_string(), "expected a single `assets` flag");
    }
    let foreign = syn::parse_quote! {
        fn read(p: &Settings, raw: Publish<'_, Bayer>) -> Result<()> { Ok(()) }
    };
    let error = expand(quote!(id = "x", foreign = disk, assets), foreign).unwrap_err();
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
            "expected Read<Payload> or Write<Payload>",
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
            "expected <Payload> or <Payload, Requirement>",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Read<A, B, C>) {}
            ),
            "expected <Payload> or <Payload, Requirement>",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Either<A, B>) {}
            ),
            "expected Read, Write, Forward, Publish, Point, Inlet or Outlet; wrapper aliases are not resolved",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Alias<A>) {}
            ),
            "expected Read, Write, Forward, Publish, Point, Inlet or Outlet; wrapper aliases are not resolved",
        ),
        (
            quote!(
                fn x(c: &C, p: &P, a: Option<Write<A>>) {}
            ),
            "output ports cannot be optional",
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
            first: ports::Write<'_, A>,
            input: ports::Read<'_, B>,
            second: ports::Write<C>,
            other: ports::Read<D>,
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
        pub fn source(context: &Context, parameters: &Params, image: Write<Image>)
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
    assert!(items.iter().any(|item| matches!(item, syn::Item::Static(item)
        if item.ident == "KIND" && matches!(item.vis, syn::Visibility::Public(_)))));
    for method in ["kind", "parameters", "identity", "contract", "run", "bind"] {
        generated_method(&module, method);
    }
    assert_eq!(literal(&module, "help"), "First line.\nSecond line.");
    let shape = kind_field(&module, "shape");
    assert_eq!(
        quote!(#shape).to_string(),
        quote!(::drip::shape::Shape::Kernel { scaled: false }).to_string()
    );
}

#[test]
fn a_kernel_claims_the_scale_with_scaled() {
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params, image: Read<Bayer>, output: Write<Rgb>) -> Result<()> { Ok(()) }
    };
    let module =
        generated_module(expand(quote!(id = "x", contract = c, scaled), function).unwrap());
    let shape = kind_field(&module, "shape");
    assert_eq!(
        quote!(#shape).to_string(),
        quote!(::drip::shape::Shape::Kernel { scaled: true }).to_string()
    );
    let message = "only a kernel claims scaled";
    for options in [
        quote!(id = "x", contract = c, warp, scaled),
        quote!(id = "x", foreign = memory, scaled),
        quote!(id = "x", contract = c, bins = BODY, scaled),
    ] {
        let function = syn::parse_quote! {
            fn x(p: &Params, image: Point<Rgb>, output: Point<Rgb>) -> Result<Warp> { Ok(()) }
        };
        assert_eq!(expand(options, function).unwrap_err().to_string(), message);
    }
    let function = syn::parse_quote! {
        fn x(c: &Context, p: &Params) -> Result<()> { Ok(()) }
    };
    let error = expand(quote!(id = "x", scaled, scaled), function).unwrap_err();
    assert_eq!(error.to_string(), "expected a single `scaled` flag");
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
    for (options, expected_name, expected_tags) in [
        (quote!(id = "raw", contract = c), "raw", quote!(&[])),
        (
            quote!(id = "raw", name = "RAW input", tags = [tag::RAW, tag::INPUT], contract = c),
            "RAW input",
            quote!(&[&tag::RAW, &tag::INPUT]),
        ),
    ] {
        let function = syn::parse_quote!(
            fn raw(context: &Context, parameters: &Params) {}
        );
        let module = generated_module(expand(options, function).unwrap());
        for (name, expected) in [("id", "raw"), ("name", expected_name)] {
            assert_eq!(literal(&module, name), expected);
        }
        let tags = kind_field(&module, "tags");
        assert_eq!(quote!(#tags).to_string(), expected_tags.to_string());
    }
}

#[test]
fn versions_default_to_one_and_are_declared_by_hand() {
    for (options, expected) in [(quote!(id = "x"), "1"), (quote!(id = "x", version = 3), "3")] {
        let function = syn::parse_quote!(
            fn x(context: &Context, parameters: &Params) {}
        );
        let module = generated_module(expand(options, function).unwrap());
        let version = kind_field(&module, "version");
        assert_eq!(quote!(#version).to_string(), expected);
    }
}

#[test]
fn foreign_nodes_identify_by_their_files_and_others_by_parameters() {
    let foreign = syn::parse_quote! {
        fn read(p: &Settings, raw: Publish<'_, Bayer>) -> Result<()> { Ok(()) }
    };
    let kernel = syn::parse_quote! { fn x(c: &Context, p: &P) -> Result<()> { Ok(()) } };
    for (function, options, expected) in [
        (
            foreign,
            quote!(id = "x", foreign = disk),
            ":: drip :: definition :: Identity :: foreign (& KIND , self . parameters () ?)",
        ),
        (
            kernel,
            quote!(id = "x"),
            "Ok (:: drip :: definition :: Identity :: Computed (self . parameters () ?))",
        ),
    ] {
        let module = generated_module(expand(options, function).unwrap());
        let body = &generated_method(&module, "identity").block;
        assert!(quote!(#body).to_string().contains(expected), "{}", quote!(#body));
    }
}

#[test]
fn optional_inputs_keep_their_port_type_and_direction_index() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn mix(
            context: &Context,
            parameters: &Params,
            image: Read<Image>,
            metadata: Option<ports::Read<'_, Metadata>>,
            output: Write<Image>,
            mask: std::option::Option<ports::Read<'_, Mask>>,
        ) {}
    };
    let signature = Signature::parse(&function.sig).unwrap();
    let inputs: Vec<_> = signature
        .inputs()
        .map(|port| {
            let ty = &port.payload;
            (port.name.to_string(), quote!(#ty).to_string(), port.optional, port.index)
        })
        .collect();
    assert_eq!(
        inputs,
        [
            ("image".into(), quote!(Image).to_string(), false, 0),
            ("metadata".into(), quote!(Metadata).to_string(), true, 1),
            ("mask".into(), quote!(Mask).to_string(), true, 2),
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
        fn x(context: &Context, parameters: &Params, image: Read<Image>, metadata: Option<Read<Metadata>>) {}
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
    assert!(image.contains("adapter :: read") && image.contains("ok_or"), "{image}");
    assert!(metadata.contains("adapter :: read") && !metadata.contains("ok_or"), "{metadata}");

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
    assert!(
        checks[0].contains(":: drip :: ports :: adapter :: input") && checks[0].contains("ok_or")
    );
    assert!(
        checks[1].contains(":: drip :: ports :: adapter :: input") && !checks[1].contains("ok_or")
    );
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
fn forwarded_outputs_publish_storage_the_node_sets() {
    let function = syn::parse_quote! {
        fn x(c: &C, p: &P, image: Read<A>, kept: Forward<B, R>, made: Write<C>) {}
    };
    let module = generated_module(expand(quote!(id = "x", contract = c), function).unwrap());
    let run = generated_method(&module, "run");
    let statements: Vec<_> = run.block.stmts.iter().map(|s| quote!(#s).to_string()).collect();
    // Inputs are materialized and bound before the output slots.
    assert!(statements[0].contains("image . materialize (context)"));
    assert!(statements[2].contains("ForwardSlot :: < B > :: new (& outputs [0usize])"));
    assert!(statements[3].contains("OutputSlot :: < C > :: new (& outputs [1usize])"));
    let call = kernel_call(run, "x");
    let arguments: Vec<_> = call.args.iter().skip(3).map(|a| quote!(#a).to_string()).collect();
    assert_eq!(
        arguments,
        [
            quote!(output_0.forward::<R>()).to_string(),
            quote!(output_1.write::<::drip::ports::Any>()).to_string()
        ]
    );
    assert_port_specs(
        &module,
        "outputs",
        quote!(
            ::drip::ports::PortSpec::of::<B, R>("kept"),
            ::drip::ports::PortSpec::of::<C, ::drip::ports::Any>("made")
        ),
    );
}

#[test]
fn port_specs_carry_requirements_and_default_to_any() {
    let function = syn::parse_quote! {
        fn x(
            context: &Context,
            parameters: &Params,
            image: Read<Rgb>,
            coefficients: Option<Read<Coefficients>>,
            working: Read<'_, Rgb, Rec2020>,
            output: Write<Rgb, Rec2020>,
        ) {}
    };
    let module = generated_module(expand(quote!(id = "x", contract = c), function).unwrap());
    for (method, expected) in [
        (
            "inputs",
            quote!(
                ::drip::ports::PortSpec::of::<Rgb, ::drip::ports::Any>("image"),
                ::drip::ports::PortSpec::of::<Coefficients, ::drip::ports::Any>("coefficients")
                    .optional(),
                ::drip::ports::PortSpec::of::<Rgb, Rec2020>("working")
            ),
        ),
        ("outputs", quote!(::drip::ports::PortSpec::of::<Rgb, Rec2020>("output"))),
    ] {
        assert_port_specs(&module, method, expected);
    }
}

#[test]
fn maps_return_constants_for_a_registered_body() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn x(
            p: &P,
            image: Point<'_, Rgb, CameraRgb>,
            metadata: Option<Read<'_, Facts>>,
            _output: Point<'_, Rgb, Rec2020>,
        ) -> Result<[f32; 9]> { Ok([0.0; 9]) }
    };
    let signature = Signature::parse(&function.sig).unwrap();
    let directions: Vec<_> = signature.ports.iter().map(|p| (p.direction, p.index)).collect();
    assert!(directions == [(Direction::Input, 0), (Direction::Input, 1), (Direction::Output, 0)]);
    let options = quote!(id = "x", contract = c, map = kernel::body);
    let module = generated_module(expand(options, function).unwrap());
    let shape = kind_field(&module, "shape");
    assert_eq!(quote!(#shape).to_string(), quote!(::drip::shape::Shape::Map(&BODY)).to_string());
    let body = module
        .content
        .as_ref()
        .unwrap()
        .1
        .iter()
        .find_map(|item| match item {
            syn::Item::Static(item) if item.ident == "BODY" => Some(quote!(#item).to_string()),
            _ => None,
        })
        .expect("generated BODY");
    assert!(body.contains("< [f32 ; 9] as :: drip :: body :: Constants > :: LEN"));
    assert!(body.contains("super :: kernel :: body :: expand"));
    assert_port_specs(
        &module,
        "inputs",
        quote!(
            ::drip::ports::PortSpec::of::<Rgb, CameraRgb>("image"),
            ::drip::ports::PortSpec::of::<Facts, ::drip::ports::Any>("metadata").optional()
        ),
    );
    assert_port_specs(
        &module,
        "outputs",
        quote!(::drip::ports::PortSpec::of::<Rgb, Rec2020>("output")),
    );
    // Only the Read input is materialized. The mapped input stays deferred.
    let run = generated_method(&module, "run");
    let run = quote!(#run).to_string();
    assert_eq!(run.matches("materialize").count(), 1);
    assert!(
        run.contains("inputs [1usize] . as_ref () . map (| image | image . materialize (context))")
    );
    assert!(run.contains(
        "image . step :: < Rgb > (:: drip :: body :: Step :: Map (& BODY , & constants) , outputs \
         [0] . clone ())"
    ));
}

#[test]
fn warps_and_resamples_publish_what_their_host_function_returns() {
    for (options, result, shape, publish) in [
        (
            quote!(id = "x", contract = c, warp),
            quote!(Warp),
            quote!(::drip::shape::Shape::Warp),
            "image . step :: < Rgb > (:: drip :: body :: Step :: Warp (& warp) , outputs [0] . clone ())",
        ),
        (
            quote!(id = "x", contract = c, resample),
            quote!(&'static FilterBody),
            quote!(::drip::shape::Shape::Resample),
            "image . step :: < Rgb > (:: drip :: body :: Step :: Resample (filter) , outputs [0] . \
             clone ())",
        ),
    ] {
        let function = syn::parse2(quote! {
            fn x(p: &P, image: Point<'_, Rgb>, metadata: Option<Read<'_, Facts>>, _output: Point<'_, Rgb>) -> Result<#result> {
                unimplemented!()
            }
        })
        .unwrap();
        let module = generated_module(expand(options, function).unwrap());
        let kind = kind_field(&module, "shape");
        assert_eq!(quote!(#kind).to_string(), shape.to_string());
        let run = generated_method(&module, "run");
        let run = quote!(#run).to_string();
        assert_eq!(run.matches("materialize").count(), 1);
        assert!(run.contains(publish), "{run}");
    }
}

#[test]
fn reductions_publish_the_host_value_their_body_computes() {
    for (options, shape, publish) in [
        (
            quote!(id = "x", contract = c, reduce = kernel::MEAN),
            quote!(::drip::shape::Shape::Reduce(&super::kernel::MEAN)),
            "image . fold :: < Rgb > (& super :: kernel :: MEAN , & constants , context) ?",
        ),
        (
            quote!(id = "x", contract = c, bins = kernel::LEVELS),
            quote!(::drip::shape::Shape::Bins(&super::kernel::LEVELS)),
            "image . count :: < Rgb > (& super :: kernel :: LEVELS , & constants , context) ?",
        ),
    ] {
        let function = syn::parse2(quote! {
            fn x(p: &P, image: Point<'_, Rgb>, metadata: Option<Read<'_, Facts>>, _out: Point<'_, Counts>) -> Result<[f32; 2]> {
                unimplemented!()
            }
        })
        .unwrap();
        let module = generated_module(expand(options, function).unwrap());
        let kind = kind_field(&module, "shape");
        assert_eq!(quote!(#kind).to_string(), shape.to_string());
        let run = generated_method(&module, "run");
        let run = quote!(#run).to_string();
        // Only the Read input is materialized. The reduced input stays deferred.
        assert_eq!(run.matches("materialize").count(), 1);
        assert!(run.contains(publish), "{run}");
        assert!(
            run.contains(":: drip :: ports :: adapter :: reduced :: < Counts , _ > (& outputs [0]"),
            "{run}"
        );
        let outputs = generated_struct(&module, "Outputs");
        let fields: Vec<_> = outputs.fields.iter().map(|f| quote!(#f).to_string()).collect();
        assert_eq!(
            fields[0],
            quote!(pub(super) out: ::drip::ports::Read<'a, Counts, ::drip::ports::Any>).to_string()
        );
    }
}

#[test]
fn stencils_publish_their_input_through_the_body_with_the_returned_reach() {
    let function = syn::parse2(quote! {
        fn x(p: &P, image: Point<'_, Rgb>, metadata: Read<'_, Facts>, _output: Point<'_, Rgb>) -> Result<Stencil<2>> {
            unimplemented!()
        }
    })
    .unwrap();
    let options = quote!(id = "x", contract = c, stencil = kernel::UNSHARP);
    let module = generated_module(expand(options, function).unwrap());
    let shape = kind_field(&module, "shape");
    let expected = quote!(::drip::shape::Shape::Stencil(&super::kernel::UNSHARP));
    assert_eq!(quote!(#shape).to_string(), expected.to_string());
    let run = generated_method(&module, "run");
    let run = quote!(#run).to_string();
    // Only the Read input is materialized here. The stencil materializes its
    // input when a consumer runs it.
    assert_eq!(run.matches("materialize").count(), 1);
    let publish = "image . step :: < Rgb > (:: drip :: body :: Step :: Stencil (& super :: kernel \
                   :: UNSHARP , stencil . radius , & stencil . constants) , outputs [0] . clone ())";
    assert!(run.contains(publish), "{run}");
}

#[test]
fn meta_nodes_expand_through_their_function_and_neither_describe_nor_run() {
    let function: syn::ItemFn = syn::parse_quote! {
        fn expand(p: &P, graph: &mut SubGraph, image: Inlet<'_, Rgb, Rec2020>, mask: Option<Inlet<'_, Mask>>, output: Outlet<Rgb>) -> Result<()> {
            Ok(())
        }
    };
    let signature = Signature::parse(&function.sig).unwrap();
    assert_eq!(signature.family, Family::Inlet);
    let module = generated_module(expand(quote!(id = "x", meta), function.clone()).unwrap());
    let shape = kind_field(&module, "shape");
    assert_eq!(quote!(#shape).to_string(), quote!(::drip::shape::Shape::Meta).to_string());
    assert_port_specs(
        &module,
        "outputs",
        quote!(::drip::ports::PortSpec::of::<Rgb, ::drip::ports::Any>("output")),
    );
    let expansion = generated_method(&module, "expand");
    let expansion = quote!(#expansion).to_string();
    for part in [
        ":: drip :: ports :: adapter :: input :: < Rgb , Rec2020 > (\"image\" , inputs [0usize] . \
         as_ref ()) ?",
        "let mut graph = :: drip :: expand :: SubGraph :: new (& KIND)",
        "super :: expand (& self . parameters , & mut graph , :: drip :: ports :: adapter :: inlet \
         :: < Rgb , Rec2020 > (0usize , image) , mask . map (| desc | :: drip :: ports :: adapter \
         :: inlet :: < Mask , :: drip :: ports :: Any > (1usize , desc)) , :: drip :: ports :: \
         adapter :: outlet :: < Rgb > (0usize)) ?",
        "graph . finish ()",
    ] {
        assert!(expansion.contains(part), "{part}\n{expansion}");
    }
    for method in ["contract", "run"] {
        let method = generated_method(&module, method);
        assert!(quote!(#method).to_string().contains("runs as its sub-graph"));
    }
    for (options, declaration, message) in [
        (
            quote!(id = "x", meta, contract = c),
            quote!(
                fn x(p: &P, g: &mut SubGraph, a: Inlet<A>, b: Outlet<A>) -> Result<()> {}
            ),
            "a meta node's sub-graph describes its outputs; remove the contract",
        ),
        (
            quote!(id = "x"),
            quote!(
                fn x(p: &P, g: &mut SubGraph, a: Inlet<A>, b: Outlet<A>) -> Result<()> {}
            ),
            "Inlet and Outlet ports declare meta",
        ),
        (
            quote!(id = "x", meta, contract = c),
            quote!(
                fn x(c: &C, p: &P, a: Read<A>, b: Write<A>) -> Result<()> {}
            ),
            "a meta node wires Inlet and Outlet ports",
        ),
        (
            quote!(id = "x", meta),
            quote!(
                fn x(p: &P, g: &mut SubGraph, a: Inlet<A>, b: Read<A>) -> Result<()> {}
            ),
            "a meta node takes &parameters, &mut SubGraph, Inlet inputs and Outlet outputs",
        ),
        (
            quote!(id = "x", meta, warp),
            quote!(
                fn x(p: &P, g: &mut SubGraph, a: Inlet<A>, b: Outlet<A>) -> Result<()> {}
            ),
            "a node is at most one of map, warp, resample, reduce, bins, stencil, foreign and meta",
        ),
        (
            quote!(id = "x", meta, scaled),
            quote!(
                fn x(p: &P, g: &mut SubGraph, a: Inlet<A>, b: Outlet<A>) -> Result<()> {}
            ),
            "only a kernel claims scaled",
        ),
    ] {
        let error = expand(options, syn::parse2(declaration).unwrap()).unwrap_err();
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn rejects_malformed_maps() {
    for (options, declaration, message) in [
        (
            quote!(id = "x", contract = c),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<[f32; 1]> {}
            ),
            "Point ports declare map = body, warp, resample, reduce = BODY, bins = BODY or \
             stencil = BODY",
        ),
        (
            quote!(id = "x", contract = c, map = body),
            quote!(
                fn x(c: &C, p: &P, a: Read<A>, b: Write<A>) {}
            ),
            "a map declares its input and output as Point ports",
        ),
        (
            quote!(id = "x", contract = c, map = body),
            quote!(
                fn x(p: &P, a: Point<A>, b: Write<A>) -> Result<[f32; 1]> {}
            ),
            "a node with Point ports has its second Point as output",
        ),
        (
            quote!(id = "x", contract = c, map = body),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>, d: Point<A>) -> Result<[f32; 1]> {}
            ),
            "a node with Point ports has one input and one output Point",
        ),
        (
            quote!(id = "x", contract = c, map = body),
            quote!(
                fn x(p: &P, a: Option<Point<A>>, b: Point<A>) -> Result<[f32; 1]> {}
            ),
            "a node with Point ports has one input and one output Point",
        ),
        (
            quote!(id = "x", contract = c, map = body),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) {}
            ),
            "a map returns Result<[f32; N]>, the constants of its body",
        ),
        (
            quote!(id = "x", contract = c, warp),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) {}
            ),
            "a warp returns Result<Warp>",
        ),
        (
            quote!(id = "x", contract = c, resample),
            quote!(
                fn x(c: &C, p: &P, a: Read<A>, b: Write<A>) {}
            ),
            "a resample declares its input and output as Point ports",
        ),
        (
            quote!(id = "x", contract = c, map = body, warp),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<[f32; 1]> {}
            ),
            "a node is at most one of map, warp, resample, reduce, bins, stencil, foreign and meta",
        ),
        (
            quote!(id = "x", contract = c, warp, warp),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<Warp> {}
            ),
            "expected a single `warp` flag",
        ),
        (
            quote!(id = "x", contract = c, map = "body"),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<[f32; 1]> {}
            ),
            "expected a #[cube] body function path",
        ),
        (
            quote!(id = "x", contract = c, reduce = "BODY"),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<[f32; 1]> {}
            ),
            "expected a ReduceBody static path",
        ),
        (
            quote!(id = "x", contract = c, bins = BODY),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) {}
            ),
            "a reduce or bins node returns Result<[f32; N]>, the constants of its body",
        ),
        (
            quote!(id = "x", contract = c, stencil = BODY),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) {}
            ),
            "a stencil returns Result<Stencil<N>>, its radius and the constants of its body",
        ),
        (
            quote!(id = "x", contract = c, stencil = "BODY"),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<Stencil<1>> {}
            ),
            "expected a StencilBody static path",
        ),
        (
            quote!(id = "x", contract = c, reduce = A, bins = B),
            quote!(
                fn x(p: &P, a: Point<A>, b: Point<A>) -> Result<[f32; 1]> {}
            ),
            "a node is at most one of map, warp, resample, reduce, bins, stencil, foreign and meta",
        ),
    ] {
        let error = expand(options, syn::parse2(declaration).unwrap()).unwrap_err();
        assert_eq!(error.to_string(), message);
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
