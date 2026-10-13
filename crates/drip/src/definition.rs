//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Error, Result,
    eval::InputValues,
    image::{Description, Image},
    param::{self, ParamSpec, Parameters},
    ports::PortSpec,
    runtime::KernelContext,
    shape::Shape,
};
use std::{
    any::Any,
    hash::{BuildHasher, RandomState},
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

/// Everything about a node kind that does not depend on its parameters. One
/// static per kind; graph checks, help and editors read the same declaration.
pub struct Kind {
    pub id: &'static str,
    /// Bumped by hand when the kind's numerics change, so outputs the store
    /// keeps from the previous version no longer match. `#[node(version = N)]`
    /// sets it, 1 by default.
    pub version: u32,
    pub name: &'static str,
    /// What the node is for, from `registry::TAGS`; structure such as ports
    /// and payloads stays in the declaration.
    pub tags: &'static [&'static Tag],
    pub help: &'static str,
    pub references: &'static [(&'static str, &'static str)],
    pub parameters: &'static [ParamSpec],
    /// Persisted default parameters.
    pub defaults: fn() -> serde_json::Value,
    /// Admit persisted parameters using this kind's typed definition.
    pub build: fn(serde_json::Value) -> Result<Arc<dyn Node>>,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    pub shape: Shape,
    /// An effect run on evaluated inputs, such as writing a file. Any frontend,
    /// headless or interactive, runs it the same way.
    pub action: Option<Action>,
}
/// A registered label for finding nodes by purpose, listed in `registry::TAGS`.
pub struct Tag {
    pub id: &'static str,
    pub description: &'static str,
    /// The durability of the outputs of nodes with this purpose.
    pub durability: Durability,
}
/// How likely an edit is to replace a stored output, in the order the store
/// evicts, `Low` first. Each tag in `registry::TAGS` states one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Durability {
    Low,
    Medium,
    High,
}
#[derive(Clone, Copy)]
pub struct Action {
    pub name: &'static str,
    /// Runs on the admitted node, which holds its parameters and assets.
    pub run: fn(&dyn Node, &InputValues) -> Result<()>,
}
/// Files a node's parameters name (`#[node(assets)]`), read when an action or
/// preparation first needs them and kept with the node, error included. Files
/// are assumed not to change; the node stays editable and persistable.
pub trait Assets {
    type Loaded: Send + Sync + 'static;
    fn load(&self) -> Result<Self::Loaded>;
}
impl Kind {
    pub fn param(&self, name: &str) -> Option<&'static ParamSpec> {
        self.parameters.iter().find(|p| p.name == name)
    }

    /// `High` for a bound source or a foreign node, otherwise the most
    /// durable of the kind's tags, `Low` without tags.
    pub fn durability(&self) -> Durability {
        if matches!(self.shape, Shape::Source | Shape::Foreign(_)) {
            return Durability::High;
        }
        self.tags.iter().map(|tag| tag.durability).max().unwrap_or(Durability::Low)
    }
}

/// What a node's outputs depend on beside its kind and its inputs.
/// Evaluation hashes it into their store keys.
#[derive(Clone, Debug, PartialEq)]
pub enum Identity {
    /// Outputs computed from these parameters, persisted JSON, under the
    /// evaluation's global context.
    Computed(serde_json::Value),
    /// Outputs a foreign node computed with these parameters from these
    /// files and its inputs. They are the same under every global context.
    Foreign(serde_json::Value, Vec<FileStamp>),
    /// Outputs bound in memory, which no other node instance shares. Keys
    /// derived from it never reach the disk.
    Bound(u128),
}
impl Identity {
    /// A foreign node's identity: its parameters and the input files they
    /// name under `kind`'s schema, stamped now. Evaluation calls it before
    /// the node reads the files, so a file written during the read stamps
    /// differently at the next evaluation. Fails, naming the path, when a
    /// file cannot be inspected.
    pub fn foreign(kind: &Kind, parameters: serde_json::Value) -> Result<Self> {
        let files = param::input_files(kind.parameters, &parameters)
            .into_iter()
            .map(FileStamp::read)
            .collect::<Result<_>>()?;
        Ok(Self::Foreign(parameters, files))
    }

    /// An identity no other call returns in this process. A random
    /// per-process seed makes a repeat in another process unlikely.
    pub fn bound() -> Self {
        static SEED: OnceLock<u64> = OnceLock::new();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let seed = *SEED.get_or_init(|| RandomState::new().hash_one(0u8));
        Self::Bound(u128::from(seed) << 64 | u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
    }
}

/// A file as a foreign node found it before reading: its path, byte length and
/// modification time. Equal stamps are taken to mean equal contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub path: PathBuf,
    pub len: u64,
    /// `None` where the platform reports no modification time.
    pub modified: Option<SystemTime>,
}
impl FileStamp {
    /// Fails, naming the path, when the file cannot be inspected.
    pub fn read(path: PathBuf) -> Result<Self> {
        let metadata = std::fs::metadata(&path)
            .map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))?;
        Ok(Self { len: metadata.len(), modified: metadata.modified().ok(), path })
    }
}

/// Mechanical adapter interface. Use #[node] for computational nodes.
/// Implementations are trusted producers, just like a Payload implementation.
pub trait Node: Any + Send + Sync {
    fn kind(&self) -> &'static Kind;
    fn parameters(&self) -> Result<serde_json::Value>;
    /// What the outputs depend on beside the kind and the inputs. A foreign
    /// node stamps its input files here, so a missing file fails here.
    fn identity(&self) -> Result<Identity>;
    /// Pure output description/check using the same global configuration as run.
    /// Inputs are the evaluated descriptions; `None` is an unconnected optional input.
    /// A foreign node describes its outputs when it runs, so its contract
    /// checks the inputs and returns no descriptions.
    fn contract(
        &self,
        global: &crate::runtime::GlobalContext,
        inputs: &[Option<Description>],
    ) -> Result<Vec<Description>>;
    /// Produce every output described by the contract, in output order.
    /// Inputs may be deferred. The node materializes those it reads as rasters.
    /// A foreign node receives materialized inputs and empty `outputs`, reads
    /// host samples only and may run on any thread.
    fn run(
        &self,
        context: &KernelContext<'_>,
        inputs: &[Option<Image>],
        outputs: &[Description],
    ) -> Result<Vec<Image>>;
}

/// A declared node type: its kind, typed parameters and typed input views.
/// Frontends bind presentation and actions through this, not through strings.
pub trait Typed: Node + Sized {
    const KIND: &'static Kind;
    type Parameters: Parameters + serde::Serialize + serde::de::DeserializeOwned;
    type Inputs<'a>;
    /// View erased input values in this node's input order and types.
    fn bind(inputs: &[Option<Image>]) -> Result<Self::Inputs<'_>>;
    /// View an admitted node as this type.
    fn of(node: &dyn Node) -> Result<&Self> {
        (node as &dyn Any)
            .downcast_ref()
            .ok_or_else(|| Error::Graph(format!("node is not {}", Self::KIND.id)))
    }
}
