//! Node parameter schemas. Parameter values are stored as JSON so that
//! persistence, graph-input arguments and unknown parameters all share one
//! representation; the schema validates them where they enter (editing,
//! loading, binding arguments) and evaluation reads them unchecked.

use std::path::Path;

use serde_json::{Map, Value as Json};

pub type ParamMap = Map<String, Json>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
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
    /// A file path; unset (`null`) by default.
    Path,
}

impl ParamKind {
    pub fn default_value(&self) -> Json {
        match *self {
            ParamKind::Float { default, .. } => default.into(),
            ParamKind::Int { default, .. } => default.into(),
            ParamKind::Bool { default } => default.into(),
            ParamKind::Choice { default, .. } => default.into(),
            ParamKind::Path => Json::Null,
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
            ParamKind::Path => value.is_null() || value.is_string(),
        }
    }
}

/// Read access to a node's validated, effective parameters during evaluation.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a>(pub(crate) &'a ParamMap);

impl Params<'_> {
    pub fn float(&self, name: &str) -> f64 {
        self.0[name].as_f64().expect("validated float")
    }

    pub fn int(&self, name: &str) -> i64 {
        self.0[name].as_i64().expect("validated int")
    }

    pub fn bool(&self, name: &str) -> bool {
        self.0[name].as_bool().expect("validated bool")
    }

    pub fn choice(&self, name: &str) -> &str {
        self.0[name].as_str().expect("validated choice")
    }

    pub fn path(&self, name: &str) -> Option<&Path> {
        self.0[name].as_str().map(Path::new)
    }
}
