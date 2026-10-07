//! Node parameter schemas. Parameter values are stored as JSON so that
//! persistence, graph-input arguments and unknown parameters all share one
//! representation; the schema validates them where they enter (editing,
//! loading, binding arguments) and evaluation reads them unchecked.

use serde_json::{Map, Value as Json};

pub type ParamMap = Map<String, Json>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    pub documentation: &'static str,
    pub name: &'static str,
    pub kind: ParamKind,
    /// Whether new nodes expose the parameter as an input of their template,
    /// i.e. expect a value per image. Users can change it per node.
    pub external: bool,
}

impl ParamSpec {
    pub const fn new(name: &'static str, kind: ParamKind) -> Self {
        ParamSpec { name, kind, external: false, documentation: "" }
    }

    pub const fn external(self) -> Self {
        ParamSpec { external: true, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamKind {
    Float {
        min: f64,
        max: f64,
        default: f64,
    },
    Int {
        min: i64,
        max: i64,
        default: i64,
    },
    Bool {
        default: bool,
    },
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
    /// A file path, unset (`null`) by default; `output` if the node writes it.
    Path {
        output: bool,
    },
}

impl ParamKind {
    pub fn default_value(&self) -> Json {
        match *self {
            ParamKind::Float { default, .. } => default.into(),
            ParamKind::Int { default, .. } => default.into(),
            ParamKind::Bool { default } => default.into(),
            ParamKind::Choice { default, .. } => default.into(),
            ParamKind::Path { .. } => Json::Null,
        }
    }

    pub fn accepts(&self, value: &Json) -> bool {
        match *self {
            ParamKind::Float { min, max, .. } => {
                value.as_f64().is_some_and(|v| (min..=max).contains(&v))
            }
            ParamKind::Int { min, max, .. } => {
                value.as_i64().is_some_and(|v| (min..=max).contains(&v))
            }
            ParamKind::Bool { .. } => value.is_boolean(),
            ParamKind::Choice { options, .. } => {
                value.as_str().is_some_and(|v| options.contains(&v))
            }
            ParamKind::Path { .. } => value.is_null() || value.is_string(),
        }
    }
}

/// Read access to a node's validated, effective parameters during evaluation.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a>(pub(crate) &'a ParamMap);

impl<'a> Params<'a> {
    /// Borrow parameters already validated by a graph or project loader.
    pub fn validated(values: &'a ParamMap) -> Self {
        Self(values)
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, name: &str) -> T {
        T::deserialize(&self.0[name]).expect("validated parameter and generated field type")
    }
}

/// Implemented by the Parameters derive; node metadata and typed reads use the
/// same fields and flattened shared configurations.
/// A schema incompatible with its field is a declaration error:
///
/// ```compile_fail,E0080
/// use drip::param::{Parameters, ParamKind};
/// #[derive(drip::Parameters)]
/// struct Bad { #[param(ParamKind::Bool { default: false })] flag: f32 }
/// const SPECS: &[drip::param::ParamSpec] = Bad::SPECS;
/// ```
/// Choice schemas must describe exactly the enum's persisted spellings:
///
/// ```compile_fail,E0080
/// use drip::param::{Parameters, ParamKind};
/// #[derive(drip::Choice)]
/// enum Mode { #[choice("linear")] Linear }
/// #[derive(drip::Parameters)]
/// struct Bad {
///     #[param(ParamKind::Choice { options: &["linear", "unknown"], default: "linear" })]
///     mode: Mode,
/// }
/// const SPECS: &[drip::param::ParamSpec] = Bad::SPECS;
/// ```
pub trait Parameters: Sized {
    const SPECS: &'static [ParamSpec];
    fn read(params: Params<'_>) -> Self;
}
impl Parameters for () {
    const SPECS: &'static [ParamSpec] = &[];
    fn read(_: Params<'_>) {}
}
#[doc(hidden)]
pub const fn concat_specs<const N: usize>(groups: &[&[ParamSpec]]) -> [ParamSpec; N] {
    let mut result = [ParamSpec::new("", ParamKind::Bool { default: false }); N];
    let (mut group, mut index) = (0, 0);
    while group < groups.len() {
        let mut field = 0;
        while field < groups[group].len() {
            let spec = groups[group][field];
            let mut prior = 0;
            while prior < index {
                assert!(
                    !same_name(result[prior].name, spec.name),
                    "duplicate flattened parameter key"
                );
                prior += 1;
            }
            result[index] = spec;
            index += 1;
            field += 1;
        }
        group += 1;
    }
    assert!(index == N);
    result
}
const fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Field/schema compatibility, checked when a generated parameter schema is built.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum FieldType {
    Float,
    Int,
    Bool,
    Choice(&'static [&'static str]),
    Path,
}

impl FieldType {
    pub const fn accepts(self, kind: &ParamKind) -> bool {
        match (self, kind) {
            (Self::Float, ParamKind::Float { .. })
            | (Self::Int, ParamKind::Int { .. })
            | (Self::Bool, ParamKind::Bool { .. })
            | (Self::Path, ParamKind::Path { .. }) => true,
            (Self::Choice(names), ParamKind::Choice { options, default }) => {
                if names.len() != options.len() {
                    return false;
                }
                let mut i = 0;
                let mut has_default = false;
                while i < names.len() {
                    if !same_name(names[i], options[i]) {
                        return false;
                    }
                    has_default |= same_name(names[i], default);
                    i += 1;
                }
                has_default
            }
            _ => false,
        }
    }
}

#[doc(hidden)]
pub trait ParameterField: serde::de::DeserializeOwned {
    const TYPE: FieldType;
}
impl ParameterField for f64 {
    const TYPE: FieldType = FieldType::Float;
}
impl ParameterField for f32 {
    const TYPE: FieldType = FieldType::Float;
}
impl ParameterField for i64 {
    const TYPE: FieldType = FieldType::Int;
}
impl ParameterField for bool {
    const TYPE: FieldType = FieldType::Bool;
}
impl ParameterField for Option<std::path::PathBuf> {
    const TYPE: FieldType = FieldType::Path;
}
