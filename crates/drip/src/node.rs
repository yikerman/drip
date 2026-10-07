//! Typed kernels and runtime descriptions for an editable graph.
//!
//! `NodeKind::new` derives parameter schemas, port contracts and evaluator
//! adapters from a kernel's associated types; names only label positions. Rust
//! checks returned types, while the graph checks user-created connections before
//! evaluation. Optional [`View`] data has its own presentation contract and is
//! not a graph output.

use crate::param::{ParamSpec, Parameters, Params};
use crate::ports::{InputRequirement, InputTuple, OutputTuple, OutputType};
use crate::resource::Resources;
use crate::value::Value;
use crate::view::{Presentation, View};
use std::collections::BTreeMap;

/// A signature determines both connection checking and evaluator adaptation.
///
/// # Laws
/// Evaluation is deterministic in parameters, inputs and evaluation context,
/// with no external writes. Outputs must obey their semantic image contracts;
/// Rust checks their types, not the pixels' meaning. These obligations allow
/// dependency stamps to reuse results without rerunning a kernel. External
/// effects such as export belong in explicit actions, outside cached evaluation.
///
/// Generated nodes derive their runtime signature from the function's Rust types.
/// Returning another interpretation fails before the node can enter a catalogue:
///
/// ```compile_fail,E0308
/// use std::sync::Arc;
/// use drip::image::{CameraRgb, Rec2020Mat};
/// use drip::node::{EvalContext, Evaluated, KernelError};
/// #[drip::node(kind = BAD, id = "bad", category = "test", name = "Bad", outputs = ["image"])]
/// fn bad(_: (), (image,): (&CameraRgb,), _: &EvalContext<'_>)
///     -> Result<Evaluated<(Arc<Rec2020Mat>,)>, KernelError>
/// {
///     Ok(Evaluated::new((Arc::new(image.clone()),)))
/// }
/// ```
pub trait NodeKernel: Sized + 'static {
    type Parameters: Parameters;
    type View: Presentation;
    type Inputs: InputTuple;
    type Outputs: OutputTuple;
    const ACTIONS: &'static [TypedAction<Self>] = &[];
    const CHECKS: &'static [TypedCheck<Self>] = &[];

    fn eval(
        params: Self::Parameters,
        inputs: <Self::Inputs as InputTuple>::Borrowed<'_>,
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs, Self::View>, KernelError>;
}

/// Incomplete configuration is normal while editing; failed processing is not.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error("{0}")]
    Incomplete(&'static str),
    #[error("{0}")]
    Failed(String),
    #[error("input {index}: {mismatch}")]
    Contract { index: usize, mismatch: crate::ports::TypeMismatch },
    #[error(transparent)]
    Constraint(ConstraintError),
}

impl From<String> for KernelError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<&str> for KernelError {
    fn from(message: &str) -> Self {
        Self::Failed(message.into())
    }
}

// K: node kernel (also used by the adapters below).
type TypedActionFn<K> = for<'inputs, 'context, 'resources> fn(
    <K as NodeKernel>::Parameters,
    <<K as NodeKernel>::Inputs as InputTuple>::Borrowed<'inputs>,
    &'context EvalContext<'resources>,
) -> Result<(), KernelError>;

/// Actions use their owning kernel's parameter and input types.
pub struct TypedAction<K>
where
    K: NodeKernel,
{
    pub name: &'static str,
    pub run: TypedActionFn<K>,
}

/// A value-dependent relationship, checked before computation and explicit actions.
/// A declaration stays pending during graph editing until its inputs are available.
pub struct TypedCheck<K: NodeKernel> {
    pub name: &'static str,
    pub check: TypedCheckFn<K>,
}
type TypedCheckFn<K> = for<'i, 'c, 'r> fn(
    <K as NodeKernel>::Parameters,
    <<K as NodeKernel>::Inputs as InputTuple>::Borrowed<'i>,
    &'c EvalContext<'r>,
) -> Result<(), String>;
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{name}: {detail}")]
pub struct ConstraintError {
    pub name: &'static str,
    pub detail: String,
}

type Evaluate =
    fn(Params<'_>, &[Option<Value>], &EvalContext<'_>) -> Result<Evaluation, KernelError>;
type RunAction =
    fn(usize, Params<'_>, &[Option<Value>], &EvalContext<'_>) -> Result<(), KernelError>;

pub struct NodeKind {
    /// Operation and assumptions, collected from the node function's Rustdoc.
    pub documentation: &'static str,
    /// Named reference links declared beside the node function.
    pub references: &'static [(&'static str, &'static str)],
    /// Offer a view surface even when evaluation is incomplete or has failed.
    has_view: bool,
    /// Stable type identity used by registries, evaluation and project files.
    pub id: &'static str,
    /// Internal grouping key, independent of type identity.
    pub category: &'static str,
    /// Default user-facing name; each graph node owns its editable copy.
    pub name: &'static str,
    pub params: &'static [ParamSpec],
    input_names: &'static [&'static str],
    output_names: &'static [&'static str],
    input_types: &'static [InputRequirement],
    output_types: &'static [OutputType],
    /// Deterministic and side-effect free: dependency stamps cache the result.
    pub(crate) eval: Evaluate,
    check_count: usize,
    check_at: fn(usize) -> &'static str,
    action_count: usize,
    action_at: fn(usize) -> Action,
}

/// A node declaration retaining its parameter type for frontend bindings.
/// The graph borrows its erased descriptor through `kind()` or deref coercion.
/// A binding expecting another parameter type cannot accept this declaration:
///
/// ```compile_fail,E0308
/// use drip::node::TypedNode;
/// fn controls(_: &TypedNode<()>) {}
/// controls(&drip::nodes::SIGMOID);
/// ```
pub struct TypedNode<P: Parameters> {
    kind: NodeKind,
    parameters: std::marker::PhantomData<fn() -> P>,
}

impl<P: Parameters> TypedNode<P> {
    pub const fn new<K: NodeKernel<Parameters = P>>(
        id: &'static str,
        category: &'static str,
        name: &'static str,
        inputs: &'static [&'static str],
        outputs: &'static [&'static str],
    ) -> Self {
        Self {
            kind: NodeKind::new::<K>(id, category, name, inputs, outputs),
            parameters: std::marker::PhantomData,
        }
    }
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }
    pub const fn documented(mut self, documentation: &'static str) -> Self {
        self.kind.documentation = documentation;
        self
    }
    pub const fn references(mut self, references: &'static [(&'static str, &'static str)]) -> Self {
        self.kind.references = references;
        self
    }
}

impl<P: Parameters> std::ops::Deref for TypedNode<P> {
    type Target = NodeKind;
    fn deref(&self) -> &NodeKind {
        self.kind()
    }
}

impl NodeKind {
    pub const fn has_view(&self) -> bool {
        self.has_view
    }
    /// The kernel's parameter type supplies the schema. Names annotate tuple
    /// positions; static declarations check arity during compilation.
    ///
    /// ```compile_fail,E0080
    /// use drip::node::{EvalContext, Evaluated, KernelError, NodeKernel, NodeKind};
    /// struct Empty;
    /// impl NodeKernel for Empty {
    ///     type Parameters = ();
    ///     type View = ();
    ///     type Inputs = ();
    ///     type Outputs = ();
    ///     fn eval(_: (), (): (), _: &EvalContext<'_>)
    ///         -> Result<Evaluated<()>, KernelError> { Ok(Evaluated::default()) }
    /// }
    /// static INVALID: NodeKind = NodeKind::new::<Empty>(
    ///     "empty", "test", "Empty", &[], &["undeclared output"],
    /// );
    /// ```
    pub const fn new<K>(
        id: &'static str,
        category: &'static str,
        name: &'static str,
        inputs: &'static [&'static str],
        outputs: &'static [&'static str],
    ) -> Self
    where
        K: NodeKernel,
    {
        assert!(inputs.len() == K::Inputs::REQUIREMENTS.len(), "input names must match tuple");
        assert!(outputs.len() == K::Outputs::TYPES.len(), "output names must match tuple");
        let mut index = 0;
        while index < K::Outputs::TYPES.len() {
            if let OutputType::Preserve(input) = K::Outputs::TYPES[index] {
                assert!(input < inputs.len(), "preserved output refers to a nonexistent input");
                assert!(
                    !K::Inputs::REQUIREMENTS[input].optional,
                    "preserved outputs need a required input"
                );
            }
            index += 1;
        }
        Self {
            check_count: K::CHECKS.len(),
            check_at: |i| K::CHECKS[i].name,
            documentation: "",
            references: &[],
            has_view: K::View::HAS_VIEW,
            id,
            category,
            name,
            params: K::Parameters::SPECS,
            input_names: inputs,
            output_names: outputs,
            input_types: K::Inputs::REQUIREMENTS,
            output_types: K::Outputs::TYPES,
            eval: evaluate::<K>,
            action_count: K::ACTIONS.len(),
            action_at: |index| Action { name: K::ACTIONS[index].name, index, run: run_action::<K> },
        }
    }
    pub const fn documented(mut self, documentation: &'static str) -> Self {
        self.documentation = documentation;
        self
    }
    pub const fn references(mut self, references: &'static [(&'static str, &'static str)]) -> Self {
        self.references = references;
        self
    }
    pub fn inputs(&self) -> impl ExactSizeIterator<Item = InputSpec> + '_ {
        self.input_names
            .iter()
            .zip(self.input_types)
            .map(|(&name, &requirement)| InputSpec { name, requirement })
    }
    pub fn outputs(&self) -> impl ExactSizeIterator<Item = OutputSpec> + '_ {
        self.output_names.iter().zip(self.output_types).map(|(&name, &ty)| OutputSpec { name, ty })
    }
    /// Named value relationships; these cannot be discharged from type IDs alone.
    pub fn checks(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        (0..self.check_count).map(self.check_at)
    }
    pub fn actions(&self) -> impl ExactSizeIterator<Item = Action> + '_ {
        (0..self.action_count).map(self.action_at)
    }
    pub fn param(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }
    pub fn input(&self, name: &str) -> Option<InputSpec> {
        self.inputs().find(|p| p.name == name)
    }
    pub fn output_index(&self, name: &str) -> Option<usize> {
        self.output_names.iter().position(|&p| p == name)
    }
    pub fn action(&self, name: &str) -> Option<Action> {
        self.actions().find(|a| a.name == name)
    }
}
impl PartialEq for NodeKind {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl std::fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id)
    }
}

#[derive(Clone, Copy)]
pub struct InputSpec {
    pub name: &'static str,
    pub requirement: InputRequirement,
}
#[derive(Debug, Clone, Copy)]
pub struct OutputSpec {
    pub name: &'static str,
    pub ty: OutputType,
}

pub struct Action {
    pub name: &'static str,
    index: usize,
    run: RunAction,
}
impl Action {
    pub fn run(
        &self,
        params: Params<'_>,
        inputs: &[Option<Value>],
        ctx: &EvalContext<'_>,
    ) -> Result<(), KernelError> {
        (self.run)(self.index, params, inputs, ctx)
    }
}

/// The presentation type determines whether frontends offer a view surface.
/// `()` means no presentation; `Option<V>` permits a temporarily absent view.
///
/// ```compile_fail,E0308
/// use drip::node::Evaluated;
/// use drip::view::View;
/// fn missing_view() -> Evaluated<(), View> {
///     Evaluated::new(())
/// }
/// ```
///
/// ```compile_fail,E0308
/// use drip::node::Evaluated;
/// use drip::view::View;
/// fn undeclared_view(view: View) -> Evaluated<()> {
///     Evaluated { outputs: (), view }
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct Evaluated<Outputs, V = ()> {
    pub outputs: Outputs,
    pub view: V,
}
impl<Outputs> Evaluated<Outputs> {
    pub fn new(outputs: Outputs) -> Self {
        Self { outputs, view: () }
    }
}
impl<V: Presentation> Evaluated<(), V> {
    pub fn view(view: V) -> Self {
        Self { outputs: (), view }
    }
}

/// Erased result cached by the heterogeneous graph executor.
pub type Evaluation = Evaluated<Vec<Value>, Option<View>>;

fn evaluate<K>(
    params: Params<'_>,
    inputs: &[Option<Value>],
    ctx: &EvalContext<'_>,
) -> Result<Evaluation, KernelError>
where
    K: NodeKernel,
{
    check_inputs::<K>(params, inputs, ctx)?;
    let result = K::eval(K::Parameters::read(params), K::Inputs::read(inputs), ctx)?;
    let outputs = result.outputs.erase();
    for (output, ty) in outputs.iter().zip(K::Outputs::TYPES) {
        if let OutputType::Preserve(index) = ty {
            let source = inputs[*index].as_ref().expect("required preserving input");
            if !output.preserves(source) {
                return Err(KernelError::Failed(
                    "kernel violated its declared interpretation preservation".into(),
                ));
            }
        }
    }
    Ok(Evaluated { outputs, view: result.view.into_view() })
}
fn run_action<K>(
    index: usize,
    params: Params<'_>,
    inputs: &[Option<Value>],
    ctx: &EvalContext<'_>,
) -> Result<(), KernelError>
where
    K: NodeKernel,
{
    check_inputs::<K>(params, inputs, ctx)?;
    (K::ACTIONS[index].run)(K::Parameters::read(params), K::Inputs::read(inputs), ctx)
}

fn check_inputs<K: NodeKernel>(
    params: Params<'_>,
    inputs: &[Option<Value>],
    ctx: &EvalContext<'_>,
) -> Result<(), KernelError> {
    if inputs.len() != K::Inputs::REQUIREMENTS.len() {
        return Err(KernelError::Failed("input arity differs from node signature".into()));
    }
    for (index, (input, requirement)) in inputs.iter().zip(K::Inputs::REQUIREMENTS).enumerate() {
        match input {
            Some(value) => requirement
                .check(value.descriptor())
                .map_err(|mismatch| KernelError::Contract { index, mismatch })?,
            None if !requirement.optional => {
                return Err(KernelError::Incomplete("missing required input"));
            }
            None => (),
        }
    }
    for check in K::CHECKS {
        (check.check)(K::Parameters::read(params), K::Inputs::read(inputs), ctx).map_err(
            |detail| KernelError::Constraint(ConstraintError { name: check.name, detail }),
        )?;
    }
    Ok(())
}

#[linkme::distributed_slice]
pub static NODE_KINDS: [&'static NodeKind];

/// What a node's evaluation may depend on besides its params and inputs.
pub struct EvalContext<'a> {
    /// Downscaled by `2^level` along each axis; frontends keep it below 32.
    pub(crate) level: u8,
    pub(crate) resources: &'a Resources,
}

impl EvalContext<'_> {
    /// Sensor pixels per image pixel along each axis; 1 is full resolution.
    /// Demosaic adapters downsample by it after sensor-space processing.
    pub fn scale(&self) -> u32 {
        1 << self.level
    }

    pub fn resources(&self) -> &Resources {
        self.resources
    }
}

/// The node kinds a graph may use.
#[derive(Default)]
pub struct Registry {
    kinds: BTreeMap<&'static str, &'static NodeKind>,
}

impl Registry {
    pub fn with(mut self, kind: &'static NodeKind) -> Self {
        assert!(self.kinds.insert(kind.id, kind).is_none(), "duplicate node type ID: {}", kind.id);
        self
    }

    pub fn get(&self, id: &str) -> Option<&'static NodeKind> {
        self.kinds.get(id).copied()
    }

    /// All kinds, ordered by type ID.
    pub fn kinds(&self) -> impl Iterator<Item = &'static NodeKind> + '_ {
        self.kinds.values().copied()
    }
}
