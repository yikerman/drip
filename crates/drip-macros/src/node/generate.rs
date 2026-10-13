use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::parse::{Direction, Options, Port, Signature};

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
    let ports = ports(signature, &vis);
    let inputs = inputs(signature, &vis);
    let (contract, run) = if signature.source {
        (loaded_contract(), loaded_run(signature))
    } else {
        (contract(options, signature), run(name, signature))
    };
    // Files are read when first forced and kept with the node, error included.
    // They are assumed not to change, so the cache keeps the node pure; any
    // parameter edit admits a new node, which reads them again.
    let (definition, admission) = if signature.source {
        let load = load(name, signature);
        (
            quote! {
                #vis struct Definition {
                    #vis parameters: #parameters,
                    loaded: ::std::sync::OnceLock<::drip::Result<Vec<::drip::image::Image>>>,
                }
                impl Definition {
                    fn loaded(&self) -> ::drip::Result<&[::drip::image::Image]> {
                        let loaded = self.loaded.get_or_init(|| load(&self.parameters));
                        loaded.as_deref().map_err(Clone::clone)
                    }
                }
                #load
            },
            quote!(Definition { parameters, loaded: Default::default() }),
        )
    } else if options.assets {
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
    let name = &options.name;
    let tags = &options.tags;
    let help = crate::documentation(&function.attrs);
    let references = &options.references;
    let parameters = &signature.parameters;
    let inputs = signature.inputs().map(port_spec);
    let outputs = signature.outputs().map(port_spec);
    let shape = if signature.source {
        quote!(::drip::definition::Shape::Source)
    } else {
        quote!(::drip::definition::Shape::Kernel)
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
                    super::#path(&node.parameters, #assets inputs.bind::<Definition>()?)
                },
            }))
        }
    };
    quote! {
        #vis static KIND: ::drip::definition::Kind = ::drip::definition::Kind {
            id: #id,
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

/// Typed input views, bound once for both the kernel and frontend extras.
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
    quote! {
        #vis struct Inputs<'a> {
            #(#fields,)*
            _lifetime: ::std::marker::PhantomData<&'a ()>,
        }

        impl ::drip::definition::Typed for Definition {
            const KIND: &'static ::drip::definition::Kind = &KIND;
            type Parameters = #parameters;
            type Inputs<'a> = Inputs<'a>;
            fn bind(inputs: &[Option<::drip::image::Image>]) -> ::drip::Result<Inputs<'_>> {
                Ok(Inputs { #(#binds,)* _lifetime: ::std::marker::PhantomData })
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
    let outputs = signature.outputs().map(|port| {
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
                let (kind, view) = if port.forward {
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
            let inputs = <Definition as ::drip::definition::Typed>::bind(inputs)?;
            #(#slots)*
            super::#name(context, &self.parameters, #(#arguments),*)?;
            Ok(vec![#(#results),*])
        }
    }
}

/// Run a source when it is first forced. A failure is cached like a value, so
/// the node stays editable and persistable and evaluation reports it.
fn load(name: &syn::Ident, signature: &Signature) -> TokenStream {
    let slots: Vec<_> =
        signature.outputs().map(|port| format_ident!("slot_{}", port.index)).collect();
    let names = signature.outputs().map(|port| port.name.to_string());
    let loads = signature.outputs().zip(&slots).map(|(port, slot)| {
        let payload = &port.payload;
        quote!(::drip::ports::Load::<#payload>::new(#slot))
    });
    let count = slots.len();
    let parameters = &signature.parameters;
    quote! {
        fn load(parameters: &#parameters) -> ::drip::Result<Vec<::drip::image::Image>> {
            let mut slots: [Option<::drip::image::Image>; #count] = ::std::array::from_fn(|_| None);
            let [#(#slots),*] = &mut slots;
            super::#name(parameters, #(#loads),*)?;
            slots
                .into_iter()
                .zip([#(#names),*])
                .map(|(slot, port)| slot.ok_or_else(|| ::drip::Error::Contract(
                    format!("source did not load {port}"),
                )))
                .collect()
        }
    }
}

fn loaded_contract() -> TokenStream {
    quote! {
        fn contract(
            &self,
            _: &::drip::runtime::GlobalContext,
            _: &[Option<::drip::image::Description>],
        ) -> ::drip::Result<Vec<::drip::image::Description>> {
            Ok(self.loaded()?.iter().map(|image| image.desc().clone()).collect())
        }
    }
}

/// Each evaluation receives the loaded host samples in a fresh raster, so a
/// device copy is the evaluation's, like any computed output.
fn loaded_run(signature: &Signature) -> TokenStream {
    let images = signature.outputs().map(|port| {
        let index = port.index;
        let payload = &port.payload;
        quote! {
            ::drip::image::Image::from(
                loaded[#index].downcast::<#payload>().expect("loaded payload").share_host()?,
            )
        }
    });
    quote! {
        fn run(
            &self,
            _: &::drip::runtime::KernelContext<'_>,
            _: &[Option<::drip::image::Image>],
            _: &[::drip::image::Description],
        ) -> ::drip::Result<Vec<::drip::image::Image>> {
            let loaded = self.loaded()?;
            Ok(vec![#(#images),*])
        }
    }
}
