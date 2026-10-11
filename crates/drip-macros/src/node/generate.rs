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
    let parameters = &signature.parameters;
    let ports = ports(signature);
    let adapter = adapter(function, options, signature);
    let registration = registration(options, parameters);

    quote! {
        #function
        #visibility mod #name {
            use super::*;

            pub struct Definition {
                pub parameters: #parameters,
            }

            #ports

            /// Admit checked parameters as a graph node.
            pub fn definition(
                parameters: #parameters,
            ) -> ::drip::Result<::std::sync::Arc<dyn ::drip::definition::Node>> {
                ::drip::param::Parameters::validate(&parameters)?;
                Ok(::std::sync::Arc::new(Definition { parameters }))
            }

            pub fn add(dag: &mut ::drip::graph::Dag, parameters: #parameters) -> ::drip::Result<Ports> {
                dag.add(definition(parameters)?).map(Ports::from_node)
            }

            #adapter
            #registration
        }
    }
}

fn payload(port: &Port) -> TokenStream {
    let ty = &port.ty;
    quote!(<#ty as ::drip::ports::Port>::Payload)
}

fn ports(signature: &Signature) -> TokenStream {
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
        fields.push(quote!(pub #name: #handle<#payload>));
        handles.push(quote!(#name: #handle::new(node, #index)));
    }
    quote! {
        pub struct Ports {
            pub node: ::drip::ports::NodeId,
            #(#fields,)*
        }

        impl Ports {
            pub fn from_node(node: ::drip::ports::NodeId) -> Self {
                Self { node, #(#handles,)* }
            }
        }
    }
}

fn adapter(function: &syn::ItemFn, options: &Options, signature: &Signature) -> TokenStream {
    let id = &options.id;
    let name = &options.name;
    let category = &options.category;
    let help = crate::documentation(&function.attrs);
    let references = &options.references;
    let parameters = &signature.parameters;
    let inputs = signature.inputs().map(port_spec);
    let outputs = signature.outputs().map(port_spec);
    let contract = contract(options, signature);
    let run = run(&function.sig.ident, signature);

    quote! {
        impl ::drip::definition::Node for Definition {
            fn metadata(&self) -> ::drip::definition::Metadata {
                ::drip::definition::Metadata {
                    id: #id,
                    name: #name,
                    category: #category,
                    help: #help,
                    references: &#references,
                    parameters: <#parameters as ::drip::param::Parameters>::SPECS,
                }
            }

            fn parameters(&self) -> ::drip::Result<::drip::__private::serde_json::Value> {
                ::drip::__private::serde_json::to_value(&self.parameters)
                    .map_err(|error| ::drip::Error::Graph(error.to_string()))
            }

            fn inputs(&self) -> Vec<::drip::ports::PortSpec> {
                vec![#(#inputs),*]
            }

            fn outputs(&self) -> Vec<::drip::ports::PortSpec> {
                vec![#(#outputs),*]
            }

            #contract
            #run
        }
    }
}

fn port_spec(port: &Port) -> TokenStream {
    let name = port.name.to_string();
    let ty = &port.ty;
    let requirement = &port.requirement;
    let spec = quote!(::drip::ports::PortSpec::of::<#ty, #requirement>(#name));
    if port.optional { quote!(#spec.optional()) } else { spec }
}

/// Generated code still errors rather than panicking if a required input is
/// absent; the graph and evaluator normally guarantee it.
fn missing(port: &Port) -> TokenStream {
    let name = port.name.to_string();
    quote!(::drip::Error::MissingInput { node: Self::ID, port: #name })
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
        let missing = missing(port);
        checks.push(if port.optional {
            quote!(let #local = #check;)
        } else {
            quote!(let #local = #check.ok_or_else(|| #missing)?;)
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
        let index = port.index;
        let ty = &port.ty;
        let requirement = &port.requirement;
        match port.direction {
            Direction::Input => {
                let read =
                    quote!(::drip::ports::Read::<#ty, #requirement>::bind(inputs[#index].as_ref()));
                let missing = missing(port);
                arguments.push(if port.optional {
                    read
                } else {
                    quote!(#read.ok_or_else(|| #missing)?)
                });
            }
            Direction::Output => {
                let local = format_ident!("output_{index}");
                allocations.push(quote! {
                    let mut #local = ::drip::ports::OutputSlot::<#ty>::allocate(&outputs[#index], context)?;
                });
                arguments.push(quote!(#local.write::<#requirement>()));
                results.push(quote!(#local.finish()?));
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
            #(#allocations)*
            super::#name(context, &self.parameters, #(#arguments),*)?;
            Ok(vec![#(#results),*])
        }
    }
}

fn registration(options: &Options, parameters: &syn::Type) -> TokenStream {
    let id = &options.id;
    quote! {
        impl Definition {
            pub const ID: &'static str = #id;
        }

        #[::drip::__private::linkme::distributed_slice(::drip::definition::NODES)]
        #[linkme(crate = ::drip::__private::linkme)]
        static REGISTRATION: ::drip::definition::Registration = ::drip::definition::Registration {
            id: #id,
            defaults: || ::drip::__private::serde_json::to_value(
                <#parameters as ::std::default::Default>::default()
            ).expect("node parameter defaults serialize"),
            build: |json| {
                let parameters = ::drip::__private::serde_json::from_value(json)
                    .map_err(|error| ::drip::Error::Contract(
                        format!("invalid parameters for {}: {error}", #id)
                    ))?;
                definition(parameters)
            },
        };
    }
}
