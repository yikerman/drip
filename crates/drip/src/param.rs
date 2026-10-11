//! UI parameter schemas generated beside typed, serde-validated settings.
//! Ranges guide controls; computational contracts decide logical validity.

use serde_json::Value as Json;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChoiceSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub parameters: Option<&'static [ParamSpec]>,
}
impl ChoiceSpec {
    pub fn default_value(&self) -> Json {
        match self.parameters {
            None => self.name.into(),
            Some(parameters) => {
                let fields: serde_json::Map<_, _> =
                    parameters.iter().map(|p| (p.name.into(), p.kind.default_value())).collect();
                serde_json::json!({ self.name: fields })
            }
        }
    }

    pub fn selected(&self, value: &Json) -> bool {
        match self.parameters {
            None => value.as_str() == Some(self.name),
            Some(_) => value.as_object().is_some_and(|v| v.len() == 1 && v.contains_key(self.name)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    pub documentation: &'static str,
    pub name: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
    pub external: bool,
}

impl ParamSpec {
    pub const fn new(name: &'static str, kind: ParamKind) -> Self {
        ParamSpec { name, label: name, kind, documentation: "", external: false }
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
        options: &'static [ChoiceSpec],
        default: &'static str,
    },
    /// A file path, unset (`null`) by default; `output` if the node writes it.
    /// Extensions name the accepted file formats, for dialogs and validation.
    Path {
        output: bool,
        extensions: &'static [&'static str],
    },
}

impl ParamKind {
    pub fn default_value(&self) -> Json {
        match *self {
            ParamKind::Float { default, .. } => default.into(),
            ParamKind::Int { default, .. } => default.into(),
            ParamKind::Bool { default } => default.into(),
            ParamKind::Choice { options, default } => options
                .iter()
                .find(|option| option.name == default)
                .expect("declared default")
                .default_value(),
            ParamKind::Path { .. } => Json::Null,
        }
    }
}

/// Typed parameters from their persisted JSON; the type is the schema check.
pub fn decode<P: serde::de::DeserializeOwned>(json: Json) -> crate::Result<P> {
    serde_json::from_value(json)
        .map_err(|e| crate::Error::Contract(format!("invalid parameters: {e}")))
}
/// The persisted JSON of typed parameters.
pub fn encode<P: serde::Serialize>(parameters: &P) -> crate::Result<Json> {
    serde_json::to_value(parameters).map_err(|e| crate::Error::Graph(e.to_string()))
}

/// Implemented by the Parameters derive; schemas follow the typed fields.
/// A schema incompatible with its field is a declaration error:
///
/// ```compile_fail,E0080
/// use drip::param::Parameters;
/// #[derive(drip::Parameters)]
/// struct Bad { #[param(open = ["icc"])] file: f32 }
/// const SPECS: &[drip::param::ParamSpec] = Bad::SPECS;
/// ```
pub trait Parameters: Sized {
    const SPECS: &'static [ParamSpec];
    /// Logical validity, checked once when parameters are admitted to a node.
    /// Schema ranges only guide controls. Set with `#[validate(path)]`.
    fn validate(&self) -> crate::Result<()> {
        Ok(())
    }
}
impl Parameters for () {
    const SPECS: &'static [ParamSpec] = &[];
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
                    if !same_name(names[i], options[i].name) {
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
