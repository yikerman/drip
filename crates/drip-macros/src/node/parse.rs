use proc_macro2::{Span, TokenStream};
use syn::{parse::Parser, spanned::Spanned};

pub(super) struct Options {
    pub id: syn::Expr,
    pub name: syn::Expr,
    /// Paths to registered `Tag` statics, e.g. `[tag::LOOK]`.
    pub tags: Vec<syn::Expr>,
    /// Required when the node has outputs; checked against the signature.
    pub contract: Option<syn::Expr>,
    pub references: syn::Expr,
    /// `fn(&Params, Inputs<'_>) -> Result<()>`, run on evaluated inputs; with
    /// `assets`, `fn(&Params, &Loaded, Inputs<'_>)`.
    pub action: Option<syn::Path>,
    /// The parameters implement `drip::definition::Assets`, loaded when first used.
    pub assets: bool,
    /// A map's `#[cube]` body, `fn(Pixel, &[f32]) -> Pixel`.
    pub map: Option<syn::Path>,
}

impl Options {
    pub fn parse(tokens: TokenStream, span: Span) -> syn::Result<Self> {
        let entries = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
            .parse2(tokens)?;
        let mut id = None;
        let mut display_name = None;
        let mut tags = None;
        let mut contract = None;
        let mut references = None;
        let mut action = None;
        let mut map = None;
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
                "tags" => &mut tags,
                "contract" => &mut contract,
                "references" => &mut references,
                "action" => &mut action,
                "map" => &mut map,
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
        let tags = match tags {
            None => Vec::new(),
            Some(syn::Expr::Array(tags)) => tags.elems.into_iter().collect(),
            Some(other) => return Err(syn::Error::new(other.span(), "expected tags = [tag, ...]")),
        };
        let function_path = |expr, role| match expr {
            syn::Expr::Path(path) => Ok(path.path),
            other => Err(syn::Error::new(other.span(), format!("expected {role} function path"))),
        };
        let action = action.map(|expr| function_path(expr, "an action")).transpose()?;
        let map = map.map(|expr| function_path(expr, "a #[cube] body")).transpose()?;
        Ok(Self {
            name: display_name.unwrap_or_else(|| id.clone()),
            tags,
            id,
            contract,
            references: references.unwrap_or_else(|| syn::parse_quote!([])),
            action,
            assets,
            map,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Input,
    Output,
}

/// The port view a node function declares.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Read,
    Write,
    /// The node sets the output from samples it holds.
    Forward,
    Load,
    /// A map's mapped input, then its output: descriptions only.
    Point,
}

pub(super) struct Port {
    /// The argument name without a leading underscore: `_image` declares port
    /// `image` on a node that does not read it.
    pub name: syn::Ident,
    pub payload: syn::Type,
    /// The port's requirement (input) or guarantee (output); `Any` if omitted.
    pub requirement: syn::Type,
    pub direction: Direction,
    pub view: View,
    pub optional: bool,
    /// Index within this direction, independent of the function argument order.
    pub index: usize,
}

pub(super) struct Signature {
    pub parameters: syn::Type,
    pub ports: Vec<Port>,
    /// `fn(&Params, Load<Payload>...)`: outputs loaded once, when first forced.
    pub source: bool,
    /// `fn(&Params, Point<In>, Read..., Point<Out>) -> Result<[f32; N]>`: the
    /// constants type `[f32; N]` of a map's host function.
    pub map: Option<syn::Type>,
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
        let source = signature.inputs.iter().any(|argument| is_view(argument, "Load"));
        let map = signature.inputs.iter().any(|argument| is_view(argument, "Point"));
        let mut arguments = signature.inputs.iter();
        if !source && !map {
            arguments.next(); // Rust checks the context type when compiling the generated call.
        }
        let parameters = arguments
            .next()
            .ok_or_else(|| syn::Error::new(signature.span(), "expected context and &parameters"))?;
        let parameters = parameter_type(parameters)?;
        let mut inputs = 0;
        let mut outputs = 0;
        let mut ports = Vec::new();
        let mut points = 0;
        for argument in arguments {
            if source != is_view(argument, "Load") {
                return Err(syn::Error::new(
                    argument.span(),
                    "a source takes &parameters and only Load ports, without a context",
                ));
            }
            let mut port = Port::parse(argument)?;
            if map && matches!(port.view, View::Write | View::Forward) {
                let message = "a map's output is its second Point, computed by the body";
                return Err(syn::Error::new(argument.span(), message));
            }
            if port.view == View::Point {
                // The first Point is the mapped input and the second the output.
                points += 1;
                if port.optional || points > 2 {
                    let message = "a map has one mapped input and one output, both Point";
                    return Err(syn::Error::new(argument.span(), message));
                }
                if points == 2 {
                    port.direction = Direction::Output;
                }
            }
            let count = match port.direction {
                Direction::Input => &mut inputs,
                Direction::Output => &mut outputs,
            };
            port.index = *count;
            *count += 1;
            ports.push(port);
        }
        let map = match map {
            false => None,
            true if points != 2 => {
                let message = "a map has one mapped input and one output, both Point";
                return Err(syn::Error::new(signature.span(), message));
            }
            true => Some(constants_type(&signature.output)?),
        };
        Ok(Self { parameters, ports, source, map })
    }

    pub fn inputs(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| port.direction == Direction::Input)
    }

    pub fn outputs(&self) -> impl Iterator<Item = &Port> {
        self.ports.iter().filter(|port| port.direction == Direction::Output)
    }
}

/// Whether `argument` is declared with the port view `view`, possibly optional.
fn is_view(argument: &syn::FnArg, view: &str) -> bool {
    let syn::FnArg::Typed(argument) = argument else { return false };
    wrapper_type(&argument.ty, argument.span()).is_ok_and(|(wrapper, types)| {
        wrapper.ident == view
            || wrapper.ident == "Option"
                && types.first().is_some_and(|ty| {
                    wrapper_type(ty, argument.span()).is_ok_and(|(inner, _)| inner.ident == view)
                })
    })
}

/// `T` in a map's `-> Result<T>`, the constants it returns.
fn constants_type(output: &syn::ReturnType) -> syn::Result<syn::Type> {
    let message = "a map returns Result<[f32; N]>, the constants of its body";
    let syn::ReturnType::Type(_, ty) = output else {
        return Err(syn::Error::new(output.span(), message));
    };
    match wrapper_type(ty, ty.span()) {
        Ok((wrapper, types)) if wrapper.ident == "Result" && types.len() == 1 => {
            Ok(types[0].clone())
        }
        _ => Err(syn::Error::new(ty.span(), message)),
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
        let (direction, view) = match wrapper.ident.to_string().as_str() {
            "Read" => (Direction::Input, View::Read),
            "Point" => (Direction::Input, View::Point),
            "Write" => (Direction::Output, View::Write),
            "Forward" => (Direction::Output, View::Forward),
            "Load" => (Direction::Output, View::Load),
            _ => {
                return Err(syn::Error::new(
                    wrapper.span(),
                    "expected Read, Write, Forward, Load or Point; wrapper aliases are not resolved",
                ));
            }
        };
        if optional && direction == Direction::Output {
            return Err(syn::Error::new(wrapper.span(), "output ports cannot be optional"));
        }
        let (payload, requirement) = match types.as_slice() {
            [ty] => ((*ty).clone(), syn::parse_quote!(::drip::ports::Any)),
            [ty, requirement] => ((*ty).clone(), (*requirement).clone()),
            _ => {
                return Err(syn::Error::new(
                    wrapper.span(),
                    "expected <Payload> or <Payload, Requirement>",
                ));
            }
        };
        if let Some(placement) = placement(&payload) {
            let message = format!(
                "ports name payloads; the {placement} placement left node signatures. \
                 Read the value with `data.host()` or `data.device(ctx)`"
            );
            return Err(syn::Error::new(payload.span(), message));
        }
        Ok(Self { name, payload, requirement, direction, view, optional, index: 0 })
    }
}

/// The wrapper's name when `ty` wraps a payload in a former `Cpu` or `Device` placement.
fn placement(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(ty) = ty else { return None };
    let segment = ty.path.segments.last()?;
    let wrapped = matches!(segment.arguments, syn::PathArguments::AngleBracketed(_));
    (wrapped && (segment.ident == "Cpu" || segment.ident == "Device"))
        .then(|| segment.ident.to_string())
}

fn wrapper_type(ty: &syn::Type, span: Span) -> syn::Result<(&syn::PathSegment, Vec<&syn::Type>)> {
    let syn::Type::Path(ty) = ty else {
        return Err(syn::Error::new(span, "expected Read<Payload> or Write<Payload>"));
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
