use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::parse::{Deferred, Direction, Foreign, Options, Port, Signature, View};

pub(super) fn node(
    function: &syn::ItemFn,
    options: &Options,
    signature: &Signature,
) -> TokenStream {
    let name = &function.sig.ident;
    let visibility = &function.vis;
    // Items are visible where the node function is: a private node's module
    // exposes them to its parent only, so private parameter types stay private.
    let vis = match visibility {
        syn::Visibility::Inherited => quote!(pub(super)),
        _ => quote!(pub),
    };
    let parameters = &signature.parameters;
    let kind = kind(function, options, signature, &vis);
    let body = match (&options.deferred, &signature.result) {
        (Some(Deferred::Map(path)), Some(constants)) => Some(body(path, constants, &vis)),
        _ => None,
    };
    let ports = ports(signature, &vis);
    let inputs = inputs(signature, &vis);
    let run = match (&options.deferred, signature.foreign) {
        (_, true) => foreign_run(name, signature),
        (Some(deferred), _) => deferred_run(name, deferred, signature),
        (None, false) => run(name, signature),
    };
    let contract = contract(options, signature);
    // A foreign node's outputs are the same at every scale. The file stamps
    // in its identity key them to the files' current contents.
    let identity = if signature.foreign {
        quote!(::drip::definition::Identity::foreign(&KIND, self.parameters()?))
    } else {
        quote!(Ok(::drip::definition::Identity::Computed(self.parameters()?)))
    };
    let (definition, admission) = if options.assets {
        let loaded = quote!(<#parameters as ::drip::definition::Assets>::Loaded);
        (
            quote! {
                #vis struct Definition {
                    #vis parameters: #parameters,
                    assets: ::std::sync::OnceLock<::drip::Result<#loaded>>,
                }
                impl Definition {
                    /// The files the parameters name, or why they could not be read.
                    #vis fn assets(&self) -> ::drip::Result<&#loaded> {
                        let assets = self.assets.get_or_init(|| {
                            ::drip::definition::Assets::load(&self.parameters)
                        });
                        assets.as_ref().map_err(Clone::clone)
                    }
                }
            },
            quote!(Definition { parameters, assets: Default::default() }),
        )
    } else {
        (
            quote! {
                #vis struct Definition {
                    #vis parameters: #parameters,
                }
            },
            quote!(Definition { parameters }),
        )
    };

    quote! {
        #function
        #visibility mod #name {
            use super::*;

            #definition
            #kind
            #body
            #ports
            #inputs

            /// Admit checked parameters as a graph node.
            #vis fn definition(
                parameters: #parameters,
            ) -> ::drip::Result<::std::sync::Arc<dyn ::drip::definition::Node>> {
                ::drip::param::Parameters::validate(&parameters)?;
                Ok(::std::sync::Arc::new(#admission))
            }

            #vis fn add(dag: &mut ::drip::graph::Dag, parameters: #parameters) -> ::drip::Result<Ports> {
                dag.add(definition(parameters)?).map(Ports::from_node)
            }

            impl ::drip::definition::Node for Definition {
                fn kind(&self) -> &'static ::drip::definition::Kind {
                    &KIND
                }

                fn parameters(&self) -> ::drip::Result<::drip::__private::serde_json::Value> {
                    ::drip::param::encode(&self.parameters)
                }

                fn identity(&self) -> ::drip::Result<::drip::definition::Identity> {
                    #identity
                }

                #contract
                #run
            }
        }
    }
}

/// The static description: metadata, schema, defaults and port declarations.
fn kind(
    function: &syn::ItemFn,
    options: &Options,
    signature: &Signature,
    vis: &TokenStream,
) -> TokenStream {
    let id = &options.id;
    let version = &options.version;
    let name = &options.name;
    let tags = &options.tags;
    let help = crate::documentation(&function.attrs);
    let references = &options.references;
    let parameters = &signature.parameters;
    let inputs = signature.inputs().map(port_spec);
    let outputs = signature.outputs().map(port_spec);
    let codecs = signature.outputs().map(|port| {
        let payload = &port.payload;
        quote!(::drip::shape::Codec::of::<#payload>())
    });
    let shape = match (options.foreign, &options.deferred) {
        (Some(Foreign::Memory), _) => {
            quote!(::drip::shape::Shape::Foreign(::drip::shape::Cache::Memory))
        }
        (Some(Foreign::Disk), _) => {
            quote!(::drip::shape::Shape::Foreign(::drip::shape::Cache::Disk(&[#(#codecs),*])))
        }
        (None, Some(Deferred::Map(_))) => quote!(::drip::shape::Shape::Map(&BODY)),
        (None, Some(Deferred::Warp)) => quote!(::drip::shape::Shape::Warp),
        (None, Some(Deferred::Resample)) => quote!(::drip::shape::Shape::Resample),
        (None, Some(Deferred::Reduce(body))) => quote!(::drip::shape::Shape::Reduce(&super::#body)),
        (None, Some(Deferred::Bins(body))) => quote!(::drip::shape::Shape::Bins(&super::#body)),
        (None, None) => {
            let scaled = options.scaled;
            quote!(::drip::shape::Shape::Kernel { scaled: #scaled })
        }
    };
    let action = match &options.action {
        None => quote!(None),
        Some(path) => {
            let label = path.segments.last().expect("action path").ident.to_string();
            let label = crate::documentation::sentence(&label);
            let assets = options.assets.then(|| quote!(node.assets()?,));
            quote!(Some(::drip::definition::Action {
                name: #label,
                run: |node, inputs| {
                    let node = <Definition as ::drip::definition::Typed>::of(node)?;
                    super::#path(&node.parameters, #assets inputs)
                },
            }))
        }
    };
    quote! {
        #vis static KIND: ::drip::definition::Kind = ::drip::definition::Kind {
            id: #id,
            version: #version,
            name: #name,
            tags: &[#(&#tags),*],
            help: #help,
            references: &#references,
            parameters: <#parameters as ::drip::param::Parameters>::SPECS,
            defaults: || ::drip::__private::serde_json::to_value(
                <#parameters as ::std::default::Default>::default()
            ).expect("node parameter defaults serialize"),
            build: |json| definition(::drip::param::decode(json)?),
            inputs: &[#(#inputs),*],
            outputs: &[#(#outputs),*],
            shape: #shape,
            action: #action,
        };
    }
}

fn port_spec(port: &Port) -> TokenStream {
    let name = port.name.to_string();
    let payload = &port.payload;
    let requirement = &port.requirement;
    let spec = quote!(::drip::ports::PortSpec::of::<#payload, #requirement>(#name));
    if port.optional { quote!(#spec.optional()) } else { spec }
}

fn ports(signature: &Signature, vis: &TokenStream) -> TokenStream {
    let mut fields = Vec::new();
    let mut handles = Vec::new();
    for port in signature.inputs().chain(signature.outputs()) {
        let name = &port.name;
        let index = port.index;
        let payload = &port.payload;
        let handle = match port.direction {
            Direction::Input => quote!(::drip::ports::Input),
            Direction::Output => quote!(::drip::ports::Output),
        };
        fields.push(quote!(#vis #name: #handle<#payload>));
        handles.push(quote!(#name: #handle::new(node, #index)));
    }
    quote! {
        #vis struct Ports {
            #vis node: ::drip::ports::NodeId,
            #(#fields,)*
        }

        impl Ports {
            #vis fn from_node(node: ::drip::ports::NodeId) -> Self {
                Self { node, #(#handles,)* }
            }
        }
    }
}

/// Typed input and output views, bound once for both the kernel and
/// frontend extras.
fn inputs(signature: &Signature, vis: &TokenStream) -> TokenStream {
    let parameters = &signature.parameters;
    let mut fields = Vec::new();
    let mut binds = Vec::new();
    for port in signature.inputs() {
        let name = &port.name;
        let index = port.index;
        let payload = &port.payload;
        let requirement = &port.requirement;
        let read = quote!(::drip::ports::Read<'a, #payload, #requirement>);
        let bind = quote!(::drip::ports::Read::bind(inputs[#index].as_ref()));
        if port.optional {
            fields.push(quote!(#vis #name: Option<#read>));
            binds.push(quote!(#name: #bind));
        } else {
            let port_name = name.to_string();
            fields.push(quote!(#vis #name: #read));
            binds.push(quote! {
                #name: #bind.ok_or(::drip::Error::MissingInput { node: KIND.id, port: #port_name })?
            });
        }
    }
    let mut output_fields = Vec::new();
    let mut output_binds = Vec::new();
    for port in signature.outputs() {
        let name = &port.name;
        let index = port.index;
        let payload = &port.payload;
        let requirement = &port.requirement;
        let port_name = name.to_string();
        output_fields.push(quote!(#vis #name: ::drip::ports::Read<'a, #payload, #requirement>));
        output_binds.push(quote! {
            #name: ::drip::ports::Read::bind(outputs.get(#index)).ok_or_else(|| {
                ::drip::Error::Contract(format!("{} has no output {}", KIND.id, #port_name))
            })?
        });
    }
    quote! {
        #vis struct Inputs<'a> {
            #(#fields,)*
            _lifetime: ::std::marker::PhantomData<&'a ()>,
        }

        #vis struct Outputs<'a> {
            #(#output_fields,)*
            _lifetime: ::std::marker::PhantomData<&'a ()>,
        }

        impl ::drip::definition::Typed for Definition {
            const KIND: &'static ::drip::definition::Kind = &KIND;
            type Parameters = #parameters;
            type Inputs<'a> = Inputs<'a>;
            type Outputs<'a> = Outputs<'a>;
            fn bind(inputs: &[Option<::drip::image::Image>]) -> ::drip::Result<Inputs<'_>> {
                Ok(Inputs { #(#binds,)* _lifetime: ::std::marker::PhantomData })
            }
            fn bind_outputs(outputs: &[::drip::image::Image]) -> ::drip::Result<Outputs<'_>> {
                Ok(Outputs { #(#output_binds,)* _lifetime: ::std::marker::PhantomData })
            }
        }
    }
}

fn contract(options: &Options, signature: &Signature) -> TokenStream {
    let mut checks = Vec::new();
    let mut arguments = Vec::new();
    for port in signature.inputs() {
        let local = &port.name;
        let name = local.to_string();
        let index = port.index;
        let payload = &port.payload;
        let requirement = &port.requirement;
        let check =
            quote!(::drip::ports::input::<#payload, #requirement>(#name, inputs[#index].as_ref())?);
        checks.push(if port.optional {
            quote!(let #local = #check;)
        } else {
            quote! {
                let #local = #check.ok_or(::drip::Error::MissingInput { node: KIND.id, port: #name })?;
            }
        });
        arguments.push(local);
    }
    // A foreign node describes its outputs when it publishes them.
    let outputs = signature.outputs().filter(|_| !signature.foreign).map(|port| {
        let name = port.name.to_string();
        let index = syn::Index::from(port.index);
        let payload = &port.payload;
        let requirement = &port.requirement;
        quote!(::drip::ports::output::<#payload, #requirement>(#name, described.#index)?)
    });
    let call = match &options.contract {
        Some(contract) => {
            quote!(let described = #contract(global, &self.parameters, #(#arguments),*)?;)
        }
        None => quote!(let _ = (global, #(#arguments),*);),
    };
    quote! {
        fn contract(
            &self,
            global: &::drip::runtime::GlobalContext,
            inputs: &[Option<::drip::image::Description>],
        ) -> ::drip::Result<Vec<::drip::image::Description>> {
            #(#checks)*
            #call
            Ok(vec![#(#outputs),*])
        }
    }
}

fn run(name: &syn::Ident, signature: &Signature) -> TokenStream {
    let mut slots = Vec::new();
    let mut arguments = Vec::new();
    let mut results = Vec::new();
    for port in &signature.ports {
        let local = &port.name;
        match port.direction {
            Direction::Input => arguments.push(quote!(inputs.#local)),
            Direction::Output => {
                let index = port.index;
                let payload = &port.payload;
                let requirement = &port.requirement;
                let slot = format_ident!("output_{index}");
                let (kind, view) = if port.view == View::Forward {
                    (quote!(ForwardSlot), quote!(forward))
                } else {
                    (quote!(OutputSlot), quote!(write))
                };
                slots.push(quote! {
                    let mut #slot = ::drip::ports::#kind::<#payload>::new(
                        ::drip::ports::typed(&outputs[#index]),
                    );
                });
                arguments.push(quote!(#slot.#view::<#requirement>()));
                results.push(quote!(#slot.finish()?));
            }
        }
    }
    quote! {
        fn run(
            &self,
            context: &::drip::runtime::KernelContext<'_>,
            inputs: &[Option<::drip::image::Image>],
            outputs: &[::drip::image::Description],
        ) -> ::drip::Result<Vec<::drip::image::Image>> {
            for image in inputs.iter().flatten() {
                image.materialize(context)?;
            }
            let inputs = <Definition as ::drip::definition::Typed>::bind(inputs)?;
            #(#slots)*
            super::#name(context, &self.parameters, #(#arguments),*)?;
            Ok(vec![#(#results),*])
        }
    }
}

/// The adapter of a map, warp, resample, reduce or bins node: materialize
/// the `Read` inputs, call the host function on descriptions, and publish
/// the deferred input extended by what it returns, or, for a reduce or bins
/// node, the host value its body computes from the input.
fn deferred_run(name: &syn::Ident, deferred: &Deferred, signature: &Signature) -> TokenStream {
    let mut binds = Vec::new();
    let mut arguments = Vec::new();
    let mut point = None;
    let mut output = None;
    for port in &signature.ports {
        let local = &port.name;
        let index = port.index;
        let payload = &port.payload;
        let requirement = &port.requirement;
        let port_name = local.to_string();
        let missing = quote!(::drip::Error::MissingInput { node: KIND.id, port: #port_name });
        binds.push(match (port.view, port.direction) {
            (View::Point, Direction::Input) => {
                point = Some((index, payload));
                quote! {
                    let #local = ::drip::ports::Point::<#payload, #requirement>::new(
                        inputs[#index].as_ref().ok_or(#missing)?.desc(),
                    );
                }
            }
            (View::Point, Direction::Output) => {
                output = Some(payload);
                quote! {
                    let #local = ::drip::ports::Point::<#payload, #requirement>::new(&outputs[#index]);
                }
            }
            _ => {
                let image = quote!(inputs[#index].as_ref());
                let read = quote!(::drip::ports::Read::<#payload, #requirement>::bind(#image));
                let read = if port.optional { read } else { quote!(#read.ok_or(#missing)?) };
                quote! {
                    #image.map(|image| image.materialize(context)).transpose()?;
                    let #local = #read;
                }
            }
        });
        arguments.push(local);
    }
    let (input, payload) = point.expect("a deferred shape has a Point input");
    let output = output.expect("a deferred shape has a Point output");
    let (result, publish) = match deferred {
        Deferred::Map(_) => (
            quote!(constants),
            quote!(image.map::<#payload>(&BODY, &constants, outputs[0].clone())),
        ),
        Deferred::Warp => (quote!(warp), quote!(image.warp::<#payload>(&warp, outputs[0].clone()))),
        Deferred::Resample => {
            (quote!(filter), quote!(image.resample::<#payload>(filter, outputs[0].clone())))
        }
        Deferred::Reduce(body) => (
            quote!(constants),
            quote!(::drip::ports::reduced::<#output, _>(
                &outputs[0],
                image.fold::<#payload>(&super::#body, &constants, context)?,
            )),
        ),
        Deferred::Bins(body) => (
            quote!(constants),
            quote!(::drip::ports::reduced::<#output, _>(
                &outputs[0],
                image.count::<#payload>(&super::#body, &constants, context)?,
            )),
        ),
    };
    quote! {
        fn run(
            &self,
            context: &::drip::runtime::KernelContext<'_>,
            inputs: &[Option<::drip::image::Image>],
            outputs: &[::drip::image::Description],
        ) -> ::drip::Result<Vec<::drip::image::Image>> {
            #(#binds)*
            let #result = super::#name(&self.parameters, #(#arguments),*)?;
            let image = inputs[#input].as_ref().expect("the Point input is bound");
            Ok(vec![#publish?])
        }
    }
}

/// A map's registered body. Its name is unique in the binary, so it can
/// identify fused kernels.
fn body(body: &syn::Path, constants: &syn::Type, vis: &TokenStream) -> TokenStream {
    quote! {
        #vis static BODY: ::drip::shape::MapBody = ::drip::shape::MapBody {
            name: concat!(module_path!(), "::", stringify!(#body)),
            constants: <#constants as ::drip::shape::Constants>::LEN,
            expand: super::#body::expand,
        };
    }
}

/// The adapter of a foreign node, run on the store job's thread with
/// materialized inputs: download them, call the host function and collect
/// what it published.
fn foreign_run(name: &syn::Ident, signature: &Signature) -> TokenStream {
    let mut downloads = Vec::new();
    let mut slots = Vec::new();
    let mut arguments = Vec::new();
    let mut results = Vec::new();
    for port in &signature.ports {
        let local = &port.name;
        match port.direction {
            Direction::Input => {
                downloads.push(if port.optional {
                    quote!(if let Some(input) = &inputs.#local { input.data.host()?; })
                } else {
                    quote!(inputs.#local.data.host()?;)
                });
                arguments.push(quote!(inputs.#local));
            }
            Direction::Output => {
                let slot = format_ident!("output_{}", port.index);
                let payload = &port.payload;
                let name = local.to_string();
                slots.push(quote!(let mut #slot = None;));
                arguments.push(quote!(::drip::ports::Publish::<#payload>::new(&mut #slot)));
                results.push(quote! {
                    #slot.ok_or_else(|| ::drip::Error::Contract(format!("node did not publish {}", #name)))?
                });
            }
        }
    }
    quote! {
        fn run(
            &self,
            _: &::drip::runtime::KernelContext<'_>,
            inputs: &[Option<::drip::image::Image>],
            _: &[::drip::image::Description],
        ) -> ::drip::Result<Vec<::drip::image::Image>> {
            let inputs = <Definition as ::drip::definition::Typed>::bind(inputs)?;
            #(#downloads)*
            #(#slots)*
            super::#name(&self.parameters, #(#arguments),*)?;
            Ok(vec![#(#results),*])
        }
    }
}
