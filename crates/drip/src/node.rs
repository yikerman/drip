//! Typed kernels and runtime descriptions for an editable graph.
//!
//! `NodeKind::new` derives parameter schemas, port contracts and evaluator
//! adapters from a declaration's associated types; names only label positions. Rust
//! checks returned types, while the graph checks user-created connections before
//! evaluation. Declarations also support input-only consumers and explicit actions.

use crate::param::{ParamSpec, Parameters, Params};
use crate::ports::{InputRequirement, InputTuple, OutputTuple};
use crate::resource::Resources;
use crate::value::{TypeDescriptor, Value};
use std::collections::BTreeMap;

/// A pure computation of context × parameters × inputs → outputs.
///
/// The signature fixes concrete payload families and CPU/GPU placement. Runtime
/// predicates refine their meaning inside the kernel: a color coordinate system
/// does not itself establish a scene measurement or an applicable noise model.
/// Implementations must explicitly preserve, transform or drop those claims.
/// External effects belong to actions; observing inputs never runs an action.
///
/// The macro binds named arguments without a separate registry or signature:
///
/// ```compile_fail,E0308
/// use drip::node::KernelError;
/// #[drip::node(kind = BAD, id = "bad", category = "test", name = "Bad", outputs = [])]
/// fn bad() -> Result<(), KernelError> { Ok(123) }
/// ```
///
/// Placement is part of the concrete signature; transferring samples is an
/// evaluator operation, not an implicit Rust cast inside a kernel:
///
/// ```compile_fail,E0308
/// use drip::{image::{ColorImage, Cpu, Gpu}, node::KernelError};
/// use std::sync::Arc;
/// #[drip::node(kind = BAD, id = "bad", category = "test", name = "Bad", outputs = ["image"])]
/// fn bad(image: &ColorImage<Gpu>) -> Result<Arc<ColorImage<Cpu>>, KernelError> {
///     Ok(Arc::new(image.clone()))
/// }
/// ```
///
/// An input-only declaration cannot promise an output without computation:
///
/// ```compile_fail,E0080
/// use drip::{image::{ColorImage, Cpu}, node::KernelError};
/// use std::sync::Arc;
/// #[drip::node(kind = BAD, id = "bad", category = "test", name = "Bad", outputs = ["image"])]
/// fn bad() -> Result<Arc<ColorImage<Cpu>>, KernelError>;
/// ```
pub trait NodeDeclaration: Sized + 'static {
    type Parameters: Parameters;
    type Inputs: InputTuple;
    type Outputs: OutputTuple;
    const ACTIONS: &'static [TypedAction<Self>] = &[];

    /// Absent for declarations consumed through actions or external observers.
    const KERNEL: Option<Kernel<Self>> = None;
}

pub type Kernel<N> = for<'i, 'c, 'r> fn(
    <N as NodeDeclaration>::Parameters,
    <<N as NodeDeclaration>::Inputs as InputTuple>::Borrowed<'i>,
    &'c EvalContext<'r>,
) -> Result<<N as NodeDeclaration>::Outputs, KernelError>;

/// Incomplete configuration is normal while editing; failed processing is not.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error("{0}")]
    Incomplete(&'static str),
    #[error("{0}")]
    Failed(String),
    #[error("input {index}: {mismatch}")]
    Contract { index: usize, mismatch: crate::ports::TypeMismatch },
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

impl From<crate::compute::ComputeError> for KernelError {
    fn from(error: crate::compute::ComputeError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<crate::image::ImageError> for KernelError {
    fn from(error: crate::image::ImageError) -> Self {
        Self::Failed(error.to_string())
    }
}

// K: owning declaration, shared by kernels and actions.
type TypedActionFn<K> = for<'inputs, 'context, 'resources> fn(
    <K as NodeDeclaration>::Parameters,
    <<K as NodeDeclaration>::Inputs as InputTuple>::Borrowed<'inputs>,
    &'context EvalContext<'resources>,
) -> Result<(), KernelError>;

/// Actions use their owning declaration's parameter and input types.
pub struct TypedAction<K>
where
    K: NodeDeclaration,
{
    pub name: &'static str,
    pub run: TypedActionFn<K>,
}

type Evaluate =
    fn(Params<'_>, &[Option<Value>], &EvalContext<'_>) -> Result<Vec<Value>, KernelError>;
type RunAction =
    fn(usize, Params<'_>, &[Option<Value>], &EvalContext<'_>) -> Result<(), KernelError>;

pub struct NodeKind {
    /// Operation and assumptions, collected from the node function's Rustdoc.
    pub documentation: &'static str,
    /// Named reference links declared beside the node function.
    pub references: &'static [(&'static str, &'static str)],
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
    output_types: &'static [TypeDescriptor],
    /// Deterministic, side-effect-free computation; results live for one evaluation.
    pub(crate) eval: Evaluate,
    action_count: usize,
    action_at: fn(usize) -> Action,
}

/// A declaration retaining its identity and full signature for typed consumers.
/// The graph borrows its erased descriptor through `kind()` or deref coercion.
/// Equal parameter types do not make two declarations interchangeable:
///
/// ```compile_fail,E0308
/// use drip::{node, node::{EvalContext, KernelError, TypedNode}};
/// #[node(kind = FIRST, id = "first", category = "test", name = "First", outputs = [])]
/// fn first() -> Result<(), KernelError>;
/// #[node(kind = SECOND, id = "second", category = "test", name = "Second", outputs = [])]
/// fn second() -> Result<(), KernelError>;
/// let _: &TypedNode<FirstNode> = &SECOND;
/// ```
pub struct TypedNode<N: NodeDeclaration> {
    kind: NodeKind,
    declaration: std::marker::PhantomData<fn() -> N>,
}

impl<N: NodeDeclaration> TypedNode<N> {
    pub const fn new(
        id: &'static str,
        category: &'static str,
        name: &'static str,
        inputs: &'static [&'static str],
        outputs: &'static [&'static str],
    ) -> Self {
        Self {
            kind: NodeKind::new::<N>(id, category, name, inputs, outputs),
            declaration: std::marker::PhantomData,
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

impl<N: NodeDeclaration> std::ops::Deref for TypedNode<N> {
    type Target = NodeKind;
    fn deref(&self) -> &NodeKind {
        self.kind()
    }
}

impl NodeKind {
    /// The declaration's parameter type supplies the schema. Names annotate tuple
    /// positions; static declarations check arity during compilation.
    ///
    pub const fn new<K>(
        id: &'static str,
        category: &'static str,
        name: &'static str,
        inputs: &'static [&'static str],
        outputs: &'static [&'static str],
    ) -> Self
    where
        K: NodeDeclaration,
    {
        assert!(K::KERNEL.is_some() || K::Outputs::TYPES.is_empty(), "outputs require a kernel");
        assert!(inputs.len() == K::Inputs::REQUIREMENTS.len(), "input names must match tuple");
        assert!(outputs.len() == K::Outputs::TYPES.len(), "output names must match tuple");
        Self {
            documentation: "",
            references: &[],
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
    pub fn inputs(&self) -> impl ExactSizeIterator<Item = InputSpec> + '_ {
        self.input_names
            .iter()
            .zip(self.input_types)
            .map(|(&name, &requirement)| InputSpec { name, requirement })
    }
    pub fn outputs(&self) -> impl ExactSizeIterator<Item = OutputSpec> + '_ {
        self.output_names.iter().zip(self.output_types).map(|(&name, &ty)| OutputSpec { name, ty })
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
    pub ty: TypeDescriptor,
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

fn evaluate<K>(
    params: Params<'_>,
    inputs: &[Option<Value>],
    ctx: &EvalContext<'_>,
) -> Result<Vec<Value>, KernelError>
where
    K: NodeDeclaration,
{
    check_inputs::<K>(params, inputs, ctx)?;
    let Some(kernel) = K::KERNEL else {
        return Ok(Vec::new());
    };
    Ok(kernel(K::Parameters::read(params), K::Inputs::read(inputs), ctx)?.erase())
}
fn run_action<K>(
    index: usize,
    params: Params<'_>,
    inputs: &[Option<Value>],
    ctx: &EvalContext<'_>,
) -> Result<(), KernelError>
where
    K: NodeDeclaration,
{
    check_inputs::<K>(params, inputs, ctx)?;
    (K::ACTIONS[index].run)(K::Parameters::read(params), K::Inputs::read(inputs), ctx)
}

pub(crate) fn check_inputs<K: NodeDeclaration>(
    _params: Params<'_>,
    inputs: &[Option<Value>],
    _ctx: &EvalContext<'_>,
) -> Result<(), KernelError> {
    if inputs.len() != K::Inputs::REQUIREMENTS.len() {
        return Err(KernelError::Failed("input arity differs from node signature".into()));
    }
    for (index, (input, requirement)) in inputs.iter().zip(K::Inputs::REQUIREMENTS).enumerate() {
        match input {
            Some(value) => requirement
                .check_exact(value.descriptor())
                .map_err(|mismatch| KernelError::Contract { index, mismatch })?,
            None if !requirement.optional => {
                return Err(KernelError::Incomplete("missing required input"));
            }
            None => (),
        }
    }
    Ok(())
}

#[linkme::distributed_slice]
pub static NODE_KINDS: [&'static NodeKind];

/// What a node's evaluation may depend on besides its params and inputs.
pub struct EvalContext<'a> {
    /// Downscaled by `2^level` along each axis; callers keep it below 31.
    pub(crate) level: u8,
    pub(crate) resources: &'a Resources,
}

impl<'a> EvalContext<'a> {
    pub fn new(level: u8, resources: &'a Resources) -> Result<Self, KernelError> {
        if level >= 31 {
            return Err(KernelError::Failed("resolution level must be below 31".into()));
        }
        Ok(Self { level, resources })
    }

    /// Sensor pixels per image pixel along each axis; 1 is full resolution.
    /// Demosaic adapters downsample by it after sensor-space processing.
    pub fn scale(&self) -> u32 {
        1 << self.level
    }

    /// The shared WGSL device and queue, initialized only when computation or
    /// transfer needs them. CPU-only resource nodes do not initialize a device.
    pub fn compute(&self) -> Result<&crate::compute::Compute, KernelError> {
        self.resources.compute().map_err(KernelError::Failed)
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
