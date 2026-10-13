use proc_macro2::{Span, TokenStream};
use syn::{parse::Parser, spanned::Spanned};

pub(super) struct Options {
    pub id: syn::Expr,
    /// `Kind::version`, 1 unless declared.
    pub version: syn::Expr,
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
    /// How a node with `Point` ports publishes its output.
    pub deferred: Option<Deferred>,
    /// `foreign = memory` or `foreign = disk`: where the store keeps the
    /// outputs of a node with `Publish` ports.
    pub foreign: Option<Foreign>,
}

/// Where a foreign node's outputs are kept beside memory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Foreign {
    Memory,
    Disk,
}

/// The shapes whose host function sees descriptions and returns what the
/// segment compiler composes.
pub(super) enum Deferred {
    /// `map = body`: a `#[cube]` body `fn(Pixel, &[f32]) -> Pixel`, with the
    /// constants the host function returns.
    Map(syn::Path),
    /// `warp`: the host function returns a `Warp`.
    Warp,
    /// `resample`: the host function returns a `&'static FilterBody`.
    Resample,
}
impl Deferred {
    /// The shape as messages name it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Map(_) => "map",
            Self::Warp => "warp",
            Self::Resample => "resample",
        }
    }
}

impl Options {
    pub fn parse(tokens: TokenStream, span: Span) -> syn::Result<Self> {
        let entries = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
            .parse2(tokens)?;
        let mut id = None;
        let mut version = None;
        let mut display_name = None;
        let mut tags = None;
        let mut contract = None;
        let mut references = None;
        let mut action = None;
        let mut map = None;
        let mut foreign = None;
        let mut flags = Vec::new();

        for entry in entries {
            let name = entry
                .path()
                .get_ident()
                .ok_or_else(|| syn::Error::new(entry.span(), "expected option name"))?;
            if ["assets", "warp", "resample"].iter().any(|flag| name == flag) {
                let flag = name.to_string();
                if flags.contains(&flag) || !matches!(entry, syn::Meta::Path(_)) {
                    let message = format!("expected a single `{flag}` flag");
                    return Err(syn::Error::new(entry.span(), message));
                }
                flags.push(flag);
                continue;
            }
            let option = match name.to_string().as_str() {
                "id" => &mut id,
                "version" => &mut version,
                "name" => &mut display_name,
                "tags" => &mut tags,
                "contract" => &mut contract,
                "references" => &mut references,
                "action" => &mut action,
                "map" => &mut map,
                "foreign" => &mut foreign,
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
        let foreign = foreign
            .map(|expr| match &expr {
                syn::Expr::Path(path) if path.path.is_ident("memory") => Ok(Foreign::Memory),
                syn::Expr::Path(path) if path.path.is_ident("disk") => Ok(Foreign::Disk),
                _ => Err(syn::Error::new(expr.span(), "expected foreign = memory or disk")),
            })
            .transpose()?;
        let flag = |name: &str| flags.iter().any(|flag| flag == name);
        let mut shapes = [
            map.map(Deferred::Map),
            flag("warp").then_some(Deferred::Warp),
            flag("resample").then_some(Deferred::Resample),
        ]
        .into_iter()
        .flatten();
        let deferred = shapes.next();
        if shapes.next().is_some() || deferred.is_some() && foreign.is_some() {
            let message = "a node is at most one of map, warp, resample and foreign";
            return Err(syn::Error::new(span, message));
        }
        Ok(Self {
            name: display_name.unwrap_or_else(|| id.clone()),
            tags,
            id,
            version: version.unwrap_or_else(|| syn::parse_quote!(1)),
            contract,
            references: references.unwrap_or_else(|| syn::parse_quote!([])),
            action,
            assets: flag("assets"),
            deferred,
            foreign,
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
    /// A foreign node's output, set with its description and host samples.
    Publish,
    /// The input, then the output, of a map, warp or resample: descriptions only.
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
    /// `fn(&Params, Read<In>..., Publish<Out>...)`: a foreign node, which
    /// describes its outputs as it publishes them.
    pub foreign: bool,
    /// `fn(&Params, Point<In>, Read..., Point<Out>) -> Result<T>`: the host
    /// function of a map, warp or resample.
    pub point: bool,
    /// `T` in the function's `-> Result<T>`, if it returns one.
    pub result: Option<syn::Type>,
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
        let foreign = signature.inputs.iter().any(|argument| is_view(argument, "Publish"));
        let point = signature.inputs.iter().any(|argument| is_view(argument, "Point"));
        let mut arguments = signature.inputs.iter();
        if !foreign && !point {
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
            if foreign && !["Read", "Publish"].iter().any(|view| is_view(argument, view)) {
                let message = "a foreign node takes &parameters, Read inputs and Publish outputs, \
                               without a context";
                return Err(syn::Error::new(argument.span(), message));
            }
            let mut port = Port::parse(argument)?;
            if point && matches!(port.view, View::Write | View::Forward) {
                let message = "a map, warp or resample has its second Point as output";
                return Err(syn::Error::new(argument.span(), message));
            }
            if port.view == View::Point {
                // The first Point is the mapped input and the second the output.
                points += 1;
                if port.optional || points > 2 {
                    let message = "a map, warp or resample has one input and one output Point";
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
        if point && points != 2 {
            let message = "a map, warp or resample has one input and one output Point";
            return Err(syn::Error::new(signature.span(), message));
        }
        let result = result_type(&signature.output);
        Ok(Self { parameters, ports, foreign, point, result })
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

/// `T` in `-> Result<T>`.
fn result_type(output: &syn::ReturnType) -> Option<syn::Type> {
    let syn::ReturnType::Type(_, ty) = output else { return None };
    match wrapper_type(ty, ty.span()) {
        Ok((wrapper, types)) if wrapper.ident == "Result" && types.len() == 1 => {
            Some(types[0].clone())
        }
        _ => None,
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
            "Publish" => (Direction::Output, View::Publish),
            _ => {
                return Err(syn::Error::new(
                    wrapper.span(),
                    "expected Read, Write, Forward, Publish or Point; wrapper aliases are not resolved",
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
