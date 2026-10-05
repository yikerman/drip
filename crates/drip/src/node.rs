//! Typed kernels and runtime descriptions for an editable graph.
//!
//! `NodeKind::new` derives port contracts and evaluator adapters from a kernel's
//! tuples; names only label positions. Rust checks returned types, while the
//! graph checks user-created connections before evaluation. Optional [`View`]
//! data has its own presentation contract and is not a graph output.

use crate::param::{ParamSpec, Params};
use crate::ports::{InputRequirement, InputTuple, OutputTuple};
use crate::resource::Resources;
use crate::value::{TypeDescriptor, Value};
use crate::view::View;
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
/// A kernel cannot return a different semantic image type:
///
/// ```compile_fail,E0308
/// use std::sync::Arc;
/// use drip::image::{DisplayRec2020, SceneRec2020, ThreeChannelMatrix};
/// use drip::node::{EvalContext, Evaluated, NodeKernel};
/// use drip::param::Params;
/// use drip::ports::Read;
/// struct IncorrectExposure;
/// impl NodeKernel for IncorrectExposure {
///     type Inputs = (Read<SceneRec2020>,);
///     type Outputs = (Arc<SceneRec2020>,);
///     fn eval(_: Params<'_>, (image,): (&SceneRec2020,), _: &EvalContext<'_>)
///         -> Result<Evaluated<Self::Outputs>, String>
///     {
///         let display = Arc::new(DisplayRec2020::from(image.rgb().clone()));
///         Ok(Evaluated::new((display,)))
///     }
/// }
/// ```
pub trait NodeKernel: Sized + 'static {
    type Inputs: InputTuple;
    type Outputs: OutputTuple;
    const ACTIONS: &'static [TypedAction<Self>] = &[];

    fn eval(
        params: Params<'_>,
        inputs: <Self::Inputs as InputTuple>::Borrowed<'_>,
        ctx: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String>;
}

// K: node kernel (also used by the adapters below).
type TypedActionFn<K> = for<'params, 'inputs, 'context, 'resources> fn(
    Params<'params>,
    <<K as NodeKernel>::Inputs as InputTuple>::Borrowed<'inputs>,
    &'context EvalContext<'resources>,
) -> Result<(), String>;

/// Actions use exactly their owning kernel's inputs; the tuple is declared once.
pub struct TypedAction<K>
where
    K: NodeKernel,
{
    pub name: &'static str,
    pub run: TypedActionFn<K>,
}

type Evaluate = fn(Params<'_>, &[Value], &EvalContext<'_>) -> Result<Evaluated, String>;
type RunAction = fn(usize, Params<'_>, &[Value], &EvalContext<'_>) -> Result<(), String>;

pub struct NodeKind {
    pub name: &'static str,
    pub label: &'static str,
    pub params: &'static [ParamSpec],
    input_names: &'static [&'static str],
    output_names: &'static [&'static str],
    input_types: &'static [InputRequirement],
    output_types: &'static [&'static TypeDescriptor],
    /// Deterministic and side-effect free: dependency stamps cache the result.
    pub(crate) eval: Evaluate,
    action_count: usize,
    action_at: fn(usize) -> Action,
}

impl NodeKind {
    /// Names annotate tuple positions. Static declarations check arity during
    /// compilation; types and the evaluator always come from the same kernel.
    ///
    /// ```compile_fail,E0080
    /// use drip::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
    /// use drip::param::Params;
    /// struct Empty;
    /// impl NodeKernel for Empty {
    ///     type Inputs = ();
    ///     type Outputs = ();
    ///     fn eval(_: Params<'_>, (): (), _: &EvalContext<'_>)
    ///         -> Result<Evaluated<()>, String> { Ok(Evaluated::default()) }
    /// }
    /// static INVALID: NodeKind = NodeKind::new::<Empty>(
    ///     "empty", "empty", &[], &[], &["undeclared output"],
    /// );
    /// ```
    pub const fn new<K>(
        name: &'static str,
        label: &'static str,
        params: &'static [ParamSpec],
        inputs: &'static [&'static str],
        outputs: &'static [&'static str],
    ) -> Self
    where
        K: NodeKernel,
    {
        assert!(inputs.len() == K::Inputs::REQUIREMENTS.len(), "input names must match tuple");
        assert!(outputs.len() == K::Outputs::TYPES.len(), "output names must match tuple");
        Self {
            name,
            label,
            params,
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
        self.name == other.name
    }
}
impl std::fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
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
    pub ty: &'static TypeDescriptor,
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
        inputs: &[Value],
        ctx: &EvalContext<'_>,
    ) -> Result<(), String> {
        (self.run)(self.index, params, inputs, ctx)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evaluated<Outputs = Vec<Value>> {
    pub outputs: Outputs,
    pub view: Option<View>,
}
impl<Outputs> Evaluated<Outputs> {
    pub fn new(outputs: Outputs) -> Self {
        Self { outputs, view: None }
    }
}
impl Evaluated<()> {
    pub fn view(view: View) -> Self {
        Self { outputs: (), view: Some(view) }
    }
}

fn evaluate<K>(
    params: Params<'_>,
    inputs: &[Value],
    ctx: &EvalContext<'_>,
) -> Result<Evaluated, String>
where
    K: NodeKernel,
{
    let result = K::eval(params, K::Inputs::read(inputs), ctx)?;
    Ok(Evaluated { outputs: result.outputs.erase(), view: result.view })
}
fn run_action<K>(
    index: usize,
    params: Params<'_>,
    inputs: &[Value],
    ctx: &EvalContext<'_>,
) -> Result<(), String>
where
    K: NodeKernel,
{
    (K::ACTIONS[index].run)(params, K::Inputs::read(inputs), ctx)
}

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
        self.kinds.insert(kind.name, kind);
        self
    }

    pub fn get(&self, name: &str) -> Option<&'static NodeKind> {
        self.kinds.get(name).copied()
    }

    /// All kinds, ordered by name.
    pub fn kinds(&self) -> impl Iterator<Item = &'static NodeKind> + '_ {
        self.kinds.values().copied()
    }
}
