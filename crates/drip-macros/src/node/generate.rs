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

            pub fn definition(parameters: #parameters) -> ::std::sync::Arc<dyn ::drip::definition::Node> {
                ::std::sync::Arc::new(Definition { parameters })
            }

            pub fn add(dag: &mut ::drip::graph::Dag, parameters: #parameters) -> ::drip::Result<Ports> {
                dag.add(definition(parameters)).map(Ports::from_node)
            }

            #adapter
            #registration
        }
    }
}

fn ports(signature: &Signature) -> TokenStream {
    let mut fields = Vec::new();
    let mut handles = Vec::new();
    for port in signature.inputs().chain(signature.outputs()) {
        let name = &port.name;
        let ty = &port.ty;
        let index = port.index;
        let handle = match port.direction {
            Direction::Input => quote!(::drip::ports::Input),
            Direction::Output => quote!(::drip::ports::Output),
        };
        fields.push(quote!(pub #name: #handle<<#ty as ::drip::ports::Port>::Payload>));
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
    let spec = quote!(::drip::ports::PortSpec::of::<#ty>(#name));
    let spec = match (&port.label, declared_payload(ty)) {
        (Some(label), _) => quote!(#spec.named_payload(#label)),
        (None, Some(payload)) => quote!(#spec.named_payload(::core::stringify!(#payload))),
        (None, None) => spec,
    };
    if port.optional { quote!(#spec.optional()) } else { spec }
}

fn declared_payload(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let wrapper = path.path.segments.last()?;
    if wrapper.ident != "Cpu" && wrapper.ident != "Device" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &wrapper.arguments else {
        return None;
    };
    let mut arguments = arguments.args.iter();
    match (arguments.next(), arguments.next()) {
        (Some(syn::GenericArgument::Type(payload)), None) => Some(payload),
        _ => None,
    }
}

fn contract(options: &Options, signature: &Signature) -> TokenStream {
    let contract = &options.contract;
    let mut descriptions = Vec::new();
    let mut erased = Vec::new();
    for port in signature.inputs() {
        let index = port.index;
        let ty = &port.ty;
        descriptions.push(quote!(
            ::drip::ports::description::<<#ty as ::drip::ports::Port>::Payload>(&inputs[#index])
        ));
    }
    for port in signature.outputs() {
        let index = syn::Index::from(port.index);
        let ty = &port.ty;
        erased.push(quote!(
            ::drip::ports::erase::<<#ty as ::drip::ports::Port>::Payload>(_described.#index)?
        ));
    }
    quote! {
        fn contract(
            &self,
            global: &::drip::runtime::GlobalContext,
            inputs: &[Option<::drip::ports::Description>],
        )
            -> ::drip::Result<Vec<Option<::drip::ports::Description>>>
        {
            let _described = #contract(global, &self.parameters, #(#descriptions),*)?;
            Ok(vec![#(#erased),*])
        }
    }
}

fn run(name: &syn::Ident, signature: &Signature) -> TokenStream {
    let arguments = signature.ports.iter().map(kernel_argument);
    let mut allocations = Vec::new();
    let mut results = Vec::new();
    for port in signature.outputs() {
        let index = port.index;
        let ty = &port.ty;
        let local = format_ident!("output_{index}");
        allocations.push(quote! {
            let mut #local = ::drip::ports::OutputSlot::<#ty>::allocate(&output_descs[#index], context)?;
        });
        results.push(quote!(Some(#local.finish()?)));
    }
    quote! {
        fn run(
            &self,
            context: &::drip::runtime::KernelContext<'_>,
            input_descs: &[Option<::drip::ports::Description>],
            inputs: &[Option<::drip::ports::Data>],
            output_descs: &[Option<::drip::ports::Description>],
        ) -> ::drip::Result<Vec<Option<::drip::ports::Data>>> {
            #(#allocations)*
            super::#name(context, &self.parameters, #(#arguments),*)?;
            Ok(vec![#(#results),*])
        }
    }
}

fn kernel_argument(port: &Port) -> TokenStream {
    let index = port.index;
    match port.direction {
        Direction::Input => read_argument(port),
        Direction::Output => {
            let local = format_ident!("output_{index}");
            quote!(#local.write())
        }
    }
}

fn read_argument(port: &Port) -> TokenStream {
    let index = port.index;
    let ty = &port.ty;
    let missing_input = format!("missing required input {}", port.name);
    let read = quote!(::drip::ports::Read::<#ty>::bind(&input_descs[#index], &inputs[#index])?);
    if port.optional {
        read
    } else {
        quote!(#read.ok_or_else(|| ::drip::Error::Contract(#missing_input.into()))?)
    }
}

fn registration(options: &Options, parameters: &syn::Type) -> TokenStream {
    let id = &options.id;
    quote! {
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
                Ok(definition(parameters))
            },
        };
    }
}
