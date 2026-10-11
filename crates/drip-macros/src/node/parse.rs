use proc_macro2::{Span, TokenStream};
use syn::{parse::Parser, spanned::Spanned};

pub(super) struct Options {
    pub id: syn::Expr,
    pub name: syn::Expr,
    pub category: syn::Expr,
    /// Required when the node has outputs; checked against the signature.
    pub contract: Option<syn::Expr>,
    pub references: syn::Expr,
    /// `fn(&Params, Inputs<'_>) -> Result<()>`, run on evaluated inputs; with
    /// `assets`, `fn(&Params, &Loaded, Inputs<'_>)`.
    pub action: Option<syn::Path>,
    /// The parameters implement `drip::definition::Assets`, loaded at admission.
    pub assets: bool,
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
        let mut action = None;
        let mut assets = false;

        for entry in entries {
            let name = entry
                .path()
                .get_ident()
                .ok_or_else(|| syn::Error::new(entry.span(), "expected option name"))?;
            if name == "assets" {
                if assets || !matches!(entry, syn::Meta::Path(_)) {
                    return Err(syn::Error::new(entry.span(), "expected a single `assets` flag"));
                }
                assets = true;
                continue;
            }
            let option = match name.to_string().as_str() {
                "id" => &mut id,
                "name" => &mut display_name,
                "category" => &mut category,
                "contract" => &mut contract,
                "references" => &mut references,
                "action" => &mut action,
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
        let action = action
            .map(|expr| match expr {
                syn::Expr::Path(path) => Ok(path.path),
                other => Err(syn::Error::new(other.span(), "expected an action function path")),
            })
            .transpose()?;
        Ok(Self {
            name: display_name.unwrap_or_else(|| id.clone()),
            category: category.unwrap_or_else(|| syn::parse_quote!("processing")),
            id,
            contract,
            references: references.unwrap_or_else(|| syn::parse_quote!([])),
            action,
            assets,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Input,
    Output,
}

pub(super) struct Port {
    /// The argument name without a leading underscore: `_image` declares port
    /// `image` on a node that does not read it.
    pub name: syn::Ident,
    pub ty: syn::Type,
    /// The port's requirement (input) or guarantee (output); `Any` if omitted.
    pub requirement: syn::Type,
    pub direction: Direction,
    pub optional: bool,
    /// Index within this direction, independent of the function argument order.
    pub index: usize,
}

pub(super) struct Signature {
    pub parameters: syn::Type,
    pub ports: Vec<Port>,
    /// `fn(&Params, Load<Payload>...)`: outputs loaded once at admission.
    pub source: bool,
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
        let source = signature.inputs.iter().any(is_load);
        let mut arguments = signature.inputs.iter();
        if !source {
            arguments.next(); // Rust checks the context type when compiling the generated call.
        }
        let parameters = arguments
            .next()
            .ok_or_else(|| syn::Error::new(signature.span(), "expected context and &parameters"))?;
        let parameters = parameter_type(parameters)?;
        let mut inputs = 0;
        let mut outputs = 0;
        let mut ports = Vec::new();
        for argument in arguments {
            if source != is_load(argument) {
                return Err(syn::Error::new(
                    argument.span(),
                    "a source takes &parameters and only Load ports, without a context",
                ));
            }
            let mut port = Port::parse(argument)?;
            let count = match port.direction {
                Direction::Input => &mut inputs,
                Direction::Output => &mut outputs,
            };
            port.index = *count;
            *count += 1;
            ports.push(port);
        }
        Ok(Self { parameters, ports, source })
    }

    pub fn inputs(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| port.direction == Direction::Input)
    }

    pub fn outputs(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| port.direction == Direction::Output)
    }
}

fn is_load(argument: &syn::FnArg) -> bool {
    let syn::FnArg::Typed(argument) = argument else { return false };
    let syn::Type::Path(ty) = argument.ty.as_ref() else { return false };
    ty.path.segments.last().is_some_and(|segment| segment.ident == "Load")
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
        let name = syn::Ident::new(name.ident.to_string().trim_start_matches('_'), name.span());
        if name == "node" {
            return Err(syn::Error::new(
                name.span(),
                "node is reserved for the generated node handle",
            ));
        }
        let (mut wrapper, mut types) = wrapper_type(&argument.ty, argument.span())?;
        let optional = wrapper.ident == "Option";
        if optional {
            let [ty] = types.as_slice() else {
                return Err(syn::Error::new(argument.span(), "expected Option<Read<..>>"));
            };
            (wrapper, types) = wrapper_type(ty, argument.span())?;
        }
        let direction = match wrapper.ident.to_string().as_str() {
            "Read" => Direction::Input,
            "Write" | "Load" => Direction::Output,
            _ => {
                return Err(syn::Error::new(
                    wrapper.span(),
                    "expected Read, Write or Load; wrapper aliases are not resolved",
                ));
            }
        };
        if optional && direction == Direction::Output {
            return Err(syn::Error::new(wrapper.span(), "output Write ports cannot be optional"));
        }
        let (ty, requirement) = match types.as_slice() {
            // A source loads host values; its ports are CPU outputs of that payload.
            [payload] if wrapper.ident == "Load" => (
                syn::parse_quote!(::drip::ports::Cpu<#payload>),
                syn::parse_quote!(::drip::ports::Any),
            ),
            [ty] => ((*ty).clone(), syn::parse_quote!(::drip::ports::Any)),
            [ty, requirement] => ((*ty).clone(), (*requirement).clone()),
            _ => {
                return Err(syn::Error::new(
                    wrapper.span(),
                    "expected <Port> or <Port, Requirement>",
                ));
            }
        };
        Ok(Self { name, ty, requirement, direction, optional, index: 0 })
    }
}

fn wrapper_type(ty: &syn::Type, span: Span) -> syn::Result<(&syn::PathSegment, Vec<&syn::Type>)> {
    let syn::Type::Path(ty) = ty else {
        return Err(syn::Error::new(span, "expected Read<Port> or Write<Port>"));
    };
    let wrapper = ty.path.segments.last().expect("type path has a segment");
    let syn::PathArguments::AngleBracketed(arguments) = &wrapper.arguments else {
        return Err(syn::Error::new(span, "expected a concrete port type"));
    };
    // Explicit view lifetimes are allowed; only type arguments describe the port.
    let types = arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            syn::GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect();
    Ok((wrapper, types))
}
