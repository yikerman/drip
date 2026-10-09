use proc_macro2::{Span, TokenStream};
use syn::{parse::Parser, spanned::Spanned};

pub(super) struct Options {
    pub id: syn::Expr,
    pub name: syn::Expr,
    pub category: syn::Expr,
    pub contract: syn::Expr,
    pub references: syn::Expr,
    pub port_labels: Option<syn::MetaList>,
}

impl Options {
    pub fn parse(tokens: TokenStream, span: Span) -> syn::Result<Self> {
        let entries = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
            .parse2(tokens)?;
        let mut id = None;
        let mut display_name = None;
        let mut category = None;
        let mut contract = None;
        let mut references = None;
        let mut port_labels = None;

        for entry in entries {
            let name = entry
                .path()
                .get_ident()
                .ok_or_else(|| syn::Error::new(entry.span(), "expected option name"))?;
            if name == "port_labels" {
                if port_labels.is_some() {
                    return Err(syn::Error::new(entry.span(), "duplicate node option"));
                }
                let syn::Meta::List(labels) = entry else {
                    return Err(syn::Error::new(
                        entry.span(),
                        "expected port_labels(port = \"label\", ...)",
                    ));
                };
                port_labels = Some(labels);
                continue;
            }
            let option = match name.to_string().as_str() {
                "id" => &mut id,
                "name" => &mut display_name,
                "category" => &mut category,
                "contract" => &mut contract,
                "references" => &mut references,
                _ => return Err(syn::Error::new(entry.span(), "unknown node option")),
            };
            if option.is_some() {
                return Err(syn::Error::new(entry.span(), "duplicate node option"));
            }
            let syn::Meta::NameValue(entry) = entry else {
                return Err(syn::Error::new(entry.span(), "expected option = value"));
            };
            *option = Some(entry.value);
        }

        let id = id.ok_or_else(|| syn::Error::new(span, "missing id"))?;
        Ok(Self {
            name: display_name.unwrap_or_else(|| id.clone()),
            category: category.unwrap_or_else(|| syn::parse_quote!("processing")),
            id,
            contract: contract.ok_or_else(|| syn::Error::new(span, "missing contract"))?,
            references: references.unwrap_or_else(|| syn::parse_quote!([])),
            port_labels,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Input,
    Output,
}

pub(super) struct Port {
    pub name: syn::Ident,
    pub ty: syn::Type,
    pub direction: Direction,
    pub optional: bool,
    pub label: Option<syn::LitStr>,
    /// Index within this direction, independent of the function argument order.
    pub index: usize,
}

pub(super) struct Signature {
    pub parameters: syn::Type,
    pub ports: Vec<Port>,
}

impl Signature {
    pub fn parse(signature: &syn::Signature) -> syn::Result<Self> {
        if signature.asyncness.is_some()
            || signature.unsafety.is_some()
            || signature.variadic.is_some()
            || !signature.generics.params.is_empty()
        {
            return Err(syn::Error::new(
                signature.span(),
                "node must be a concrete synchronous safe function",
            ));
        }
        let mut arguments = signature.inputs.iter();
        arguments.next(); // Rust checks the context type when compiling the generated call.
        let parameters = arguments
            .next()
            .ok_or_else(|| syn::Error::new(signature.span(), "expected context and &parameters"))?;
        let parameters = parameter_type(parameters)?;
        let mut inputs = 0;
        let mut outputs = 0;
        let mut ports = Vec::new();
        for argument in arguments {
            let mut port = Port::parse(argument)?;
            let count = match port.direction {
                Direction::Input => &mut inputs,
                Direction::Output => &mut outputs,
            };
            port.index = *count;
            *count += 1;
            ports.push(port);
        }
        Ok(Self { parameters, ports })
    }

    pub fn inputs(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| port.direction == Direction::Input)
    }

    pub fn label_ports(&mut self, labels: &Option<syn::MetaList>) -> syn::Result<()> {
        let Some(labels) = labels else { return Ok(()) };
        let entries = labels.parse_args_with(
            syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated,
        )?;
        for entry in entries {
            let name = entry
                .path
                .get_ident()
                .ok_or_else(|| syn::Error::new(entry.path.span(), "expected port name"))?;
            let port = self
                .ports
                .iter_mut()
                .find(|port| port.name == *name)
                .ok_or_else(|| syn::Error::new(name.span(), "unknown port name"))?;
            if port.label.is_some() {
                return Err(syn::Error::new(name.span(), "duplicate port label"));
            }
            let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(label), .. }) = entry.value else {
                return Err(syn::Error::new(
                    entry.value.span(),
                    "port label must be a string literal",
                ));
            };
            port.label = Some(label);
        }
        Ok(())
    }

    pub fn outputs(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| port.direction == Direction::Output)
    }
}

fn parameter_type(argument: &syn::FnArg) -> syn::Result<syn::Type> {
    let syn::FnArg::Typed(argument) = argument else {
        return Err(syn::Error::new(argument.span(), "expected &parameters"));
    };
    let syn::Type::Reference(reference) = argument.ty.as_ref() else {
        return Err(syn::Error::new(argument.span(), "parameters must be borrowed"));
    };
    if reference.mutability.is_some() {
        return Err(syn::Error::new(reference.span(), "parameters must be immutable"));
    }
    Ok(*reference.elem.clone())
}

impl Port {
    fn parse(argument: &syn::FnArg) -> syn::Result<Self> {
        let syn::FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new(argument.span(), "expected a named port"));
        };
        let syn::Pat::Ident(name) = argument.pat.as_ref() else {
            return Err(syn::Error::new(argument.span(), "ports must be named"));
        };
        if name.ident == "node" {
            return Err(syn::Error::new(
                name.span(),
                "node is reserved for the generated node handle",
            ));
        }
        let (mut wrapper, mut ty) = wrapper_type(&argument.ty, argument.span())?;
        let optional = wrapper.ident == "Option";
        if optional {
            (wrapper, ty) = wrapper_type(ty, argument.span())?;
        }
        let direction = match wrapper.ident.to_string().as_str() {
            "Read" => Direction::Input,
            "Write" => Direction::Output,
            _ => {
                return Err(syn::Error::new(
                    wrapper.span(),
                    "expected Read or Write; wrapper aliases are not resolved",
                ));
            }
        };
        if optional && direction == Direction::Output {
            return Err(syn::Error::new(wrapper.span(), "output Write ports cannot be optional"));
        }
        Ok(Self {
            name: name.ident.clone(),
            ty: ty.clone(),
            direction,
            optional,
            label: None,
            index: 0,
        })
    }
}

fn wrapper_type(ty: &syn::Type, span: Span) -> syn::Result<(&syn::PathSegment, &syn::Type)> {
    let syn::Type::Path(ty) = ty else {
        return Err(syn::Error::new(span, "expected Read<Port> or Write<Port>"));
    };
    let wrapper = ty.path.segments.last().expect("type path has a segment");
    let syn::PathArguments::AngleBracketed(arguments) = &wrapper.arguments else {
        return Err(syn::Error::new(span, "expected a concrete port type"));
    };
    // Explicit view lifetimes are allowed; only type arguments describe the port.
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    match (types.next(), types.next()) {
        (Some(ty), None) => Ok((wrapper, ty)),
        _ => Err(syn::Error::new(span, "expected exactly one concrete port type")),
    }
}
