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
        (loaded_contract(), loaded_run())
    } else {
        (contract(options, signature), run(name, signature))
    };
    let (definition, admission) = if signature.source {
        let load = load(name, signature);
        (
            quote! {
                #vis struct Definition {
                    #vis parameters: #parameters,
                    loaded: ::drip::Result<Vec<(::drip::ports::Description, ::drip::ports::Data)>>,
                }
                #load
            },
            quote!(Definition { loaded: load(&parameters), parameters }),
        )
    } else if options.assets {
        let loaded = quote!(<#parameters as ::drip::definition::Assets>::Loaded);
        (
            quote! {
                #vis struct Definition {
                    #vis parameters: #parameters,
                    assets: ::drip::Result<#loaded>,
                }
                impl Definition {
                    /// The files loaded at admission, or why they could not be.
                    #vis fn assets(&self) -> ::drip::Result<&#loaded> {
                        self.assets.as_ref().map_err(Clone::clone)
                    }
                }
            },
            quote! {
                Definition {
                    assets: ::drip::definition::Assets::load(&parameters),
                    parameters,
                }
            },
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

            #[::drip::__private::linkme::distributed_slice(::drip::definition::NODES)]
            #[linkme(crate = ::drip::__private::linkme)]
            static REGISTRATION: ::drip::definition::Registration =
                ::drip::definition::Registration {
                    kind: &KIND,
                    build: |json| definition(::drip::param::decode(json)?),
                };
        }
    }
}

fn payload(port: &Port) -> TokenStream {
    let ty = &port.ty;
    quote!(<#ty as ::drip::ports::Port>::Payload)
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
    let category = &options.category;
    let help = crate::documentation(&function.attrs);
    let references = &options.references;
    let parameters = &signature.parameters;
    let loads_assets = signature.source || options.assets;
    let inputs = signature.inputs().map(port_spec);
    let outputs = signature.outputs().map(port_spec);
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
            category: #category,
            help: #help,
            references: &#references,
            parameters: <#parameters as ::drip::param::Parameters>::SPECS,
            defaults: || ::drip::__private::serde_json::to_value(
                <#parameters as ::std::default::Default>::default()
            ).expect("node parameter defaults serialize"),
            loads_assets: #loads_assets,
            inputs: &[#(#inputs),*],
            outputs: &[#(#outputs),*],
            action: #action,
        };
    }
}

fn port_spec(port: &Port) -> TokenStream {
    let name = port.name.to_string();
    let ty = &port.ty;
    let requirement = &port.requirement;
    let spec = quote!(::drip::ports::PortSpec::of::<#ty, #requirement>(#name));
    if port.optional { quote!(#spec.optional()) } else { spec }
}

fn ports(signature: &Signature, vis: &TokenStream) -> TokenStream {
    let mut fields = Vec::new();
    let mut handles = Vec::new();
    for port in signature.inputs().chain(signature.outputs()) {
        let name = &port.name;
        let index = port.index;
        let payload = payload(port);
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
        let ty = &port.ty;
        let requirement = &port.requirement;
        let read = quote!(::drip::ports::Read<'a, #ty, #requirement>);
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
            fn bind(
                inputs: &[Option<(::drip::ports::Description, ::drip::ports::Data)>],
            ) -> ::drip::Result<Inputs<'_>> {
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
        let ty = &port.ty;
        let requirement = &port.requirement;
        let check =
            quote!(::drip::ports::input::<#ty, #requirement>(#name, inputs[#index].as_ref())?);
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
        let ty = &port.ty;
        let requirement = &port.requirement;
        quote!(::drip::ports::output::<#ty, #requirement>(#name, described.#index)?)
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
            inputs: &[Option<::drip::ports::Description>],
        ) -> ::drip::Result<::drip::definition::Outcome> {
            #(#checks)*
            #call
            Ok(::drip::definition::Outcome::Known(vec![#(#outputs),*]))
        }
    }
}

fn run(name: &syn::Ident, signature: &Signature) -> TokenStream {
    let mut allocations = Vec::new();
    let mut arguments = Vec::new();
    let mut results = Vec::new();
    for port in &signature.ports {
        let local = &port.name;
        match port.direction {
            Direction::Input => arguments.push(quote!(inputs.#local)),
            Direction::Output => {
                let index = port.index;
                let ty = &port.ty;
                let requirement = &port.requirement;
                let slot = format_ident!("output_{index}");
                allocations.push(quote! {
                    let mut #slot = ::drip::ports::OutputSlot::<#ty>::allocate(&outputs[#index], context)?;
                });
                arguments.push(quote!(#slot.write::<#requirement>()));
                results.push(quote!(#slot.finish()?));
            }
        }
    }
    quote! {
        fn run(
            &self,
            context: &::drip::runtime::KernelContext<'_>,
            inputs: &[Option<(::drip::ports::Description, ::drip::ports::Data)>],
            outputs: &[::drip::ports::Description],
        ) -> ::drip::Result<Vec<::drip::ports::Data>> {
            let inputs = <Definition as ::drip::definition::Typed>::bind(inputs)?;
            #(#allocations)*
            super::#name(context, &self.parameters, #(#arguments),*)?;
            Ok(vec![#(#results),*])
        }
    }
}

/// Run a source once at admission. A failure is kept as the reason its outputs
/// are unknown, so the node stays editable and persistable.
fn load(name: &syn::Ident, signature: &Signature) -> TokenStream {
    let slots: Vec<_> =
        signature.outputs().map(|port| format_ident!("slot_{}", port.index)).collect();
    let names = signature.outputs().map(|port| port.name.to_string());
    let loads = signature.outputs().zip(&slots).map(|(port, slot)| {
        let payload = payload(port);
        quote!(::drip::ports::Load::<#payload>::new(#slot))
    });
    let count = slots.len();
    let parameters = &signature.parameters;
    quote! {
        fn load(
            parameters: &#parameters,
        ) -> ::drip::Result<Vec<(::drip::ports::Description, ::drip::ports::Data)>> {
            let mut slots: [Option<(::drip::ports::Description, ::drip::ports::Data)>; #count] =
                ::std::array::from_fn(|_| None);
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
            _: &[Option<::drip::ports::Description>],
        ) -> ::drip::Result<::drip::definition::Outcome> {
            Ok(match &self.loaded {
                Ok(values) => ::drip::definition::Outcome::Known(
                    values.iter().map(|(desc, _)| desc.clone()).collect(),
                ),
                Err(reason) => ::drip::definition::Outcome::Unknown(reason.clone()),
            })
        }
    }
}

fn loaded_run() -> TokenStream {
    quote! {
        fn run(
            &self,
            _: &::drip::runtime::KernelContext<'_>,
            _: &[Option<(::drip::ports::Description, ::drip::ports::Data)>],
            _: &[::drip::ports::Description],
        ) -> ::drip::Result<Vec<::drip::ports::Data>> {
            let values = self.loaded.as_ref().map_err(Clone::clone)?;
            Ok(values.iter().map(|(_, data)| data.clone()).collect())
        }
    }
}
