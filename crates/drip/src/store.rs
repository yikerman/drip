//! The evaluator's memo: materialized outputs kept across demands, keyed by
//! content.
//!
//! An edit usually changes a few nodes, so most outputs of the next demand
//! equal outputs the evaluator already computed. Evaluation runs a node only
//! when a demanded output is missing here. The store also runs foreign nodes
//! as jobs and keeps the outputs of `foreign = disk` nodes in a [`Disk`]
//! directory that outlives the process.
//!
//! # Rules
//!
//! - **Keys.** An output's key is a SHA-256 over the producer's kind ID and
//!   `Kind::version`, its [`Identity`], the requested scale when the kind's
//!   shape is scaled, and the keys of its inputs in port order. A computed
//!   node's identity is its parameter JSON with object keys sorted. A foreign
//!   node's is its parameters and the stamps (path, length, modification
//!   time) of the input files they name, without the scale. Bound data has a
//!   random identity per instance, and keys derived from it never reach the
//!   disk. An image reduced to the requested scale and the part a region
//!   covers have keys derived from the image's. Equal keys mean equal
//!   samples, so equal nodes in two graphs share an entry.
//! - **Entries.** Entries are materialized rasters, never replaced. A
//!   deferred image enters once a consumer has materialized it, so the store
//!   never holds a plan's input.
//! - **Eviction.** Each side stays within the runtime's [`Budget`], device
//!   first. Over a budget the store drops a representation from the least
//!   [`Durability`], least recently used entry. Dropping the device side
//!   keeps the host one, and an entry with neither leaves. A kind's
//!   durability is `High` for foreign nodes and bound data, otherwise the
//!   most durable of its tags, `Low` without tags.
//! - **Jobs.** One thread per foreign node key. A failed job stays, so its
//!   key repeats the error without running again. A job writes its disk files
//!   before it calls the wake callback.
//! - **Disk.** One file per output, named by its key in hexadecimal, holding
//!   the line `drip store 1`, the payload name and the payload's `Persist`
//!   stream. Files are renamed into place, a file that fails to read is
//!   deleted and counts as a miss, and over the budget the least recently
//!   used files go first.
//! - **Expansions.** The expansions of the 64 meta nodes used last, by the
//!   meta node's key.
use crate::{
    Error, Result,
    definition::{Durability, FileStamp, Identity, Kind},
    expand::SubGraph,
    image::{Image, Side},
    lattice::Rect,
    payload::{read_json, write_json},
    runtime::Budget,
    shape::{Cache, Codec},
};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

/// A SHA-256 content key. Outputs with equal keys hold equal samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    hash: [u8; 32],
    /// Derived from a bound identity, which is random per process, so the
    /// key means nothing to another process and stays off the disk.
    local: bool,
}

impl Key {
    /// The key of a node's outputs: `kind` and its version, `identity`, the
    /// requested `scale` for a kind whose shape is scaled, and the key of
    /// each input in port order, `None` where the input is unconnected.
    pub fn node(kind: &Kind, identity: &Identity, scale: u32, inputs: &[Option<Key>]) -> Self {
        let mut hash = Hasher::new(b"drip node");
        let local = matches!(identity, Identity::Bound(_))
            || inputs.iter().flatten().any(|input| input.local);
        hash.bytes(kind.id.as_bytes());
        hash.u64(kind.version.into());
        match identity {
            Identity::Computed(parameters) => {
                hash.tag(0);
                hash.json(parameters);
                if kind.shape.scaled() {
                    hash.u64(scale.into());
                }
            }
            Identity::Foreign(parameters, files) => {
                hash.tag(1);
                hash.json(parameters);
                hash.u64(files.len() as u64);
                files.iter().for_each(|file| hash.file(file));
            }
            Identity::Bound(instance) => {
                hash.tag(2);
                hash.0.update(instance.to_le_bytes());
            }
        }
        hash.u64(inputs.len() as u64);
        for input in inputs {
            match input {
                None => hash.tag(0),
                Some(key) => {
                    hash.tag(1);
                    hash.0.update(key.hash);
                }
            }
        }
        hash.finish(local)
    }

    /// The key of output `index` of the node keyed `self`.
    pub fn output(self, index: usize) -> Self {
        let mut hash = Hasher::new(b"drip output");
        hash.0.update(self.hash);
        hash.u64(index as u64);
        hash.finish(self.local)
    }

    /// The key of the image keyed `self` reduced to `scale`.
    pub fn reduced(self, scale: u32) -> Self {
        let mut hash = Hasher::new(b"drip reduced");
        hash.0.update(self.hash);
        hash.u64(scale.into());
        hash.finish(self.local)
    }

    /// The key of the samples of the image keyed `self` that cover `region`
    /// of its plane, in full-detail pixels.
    pub fn region(self, region: Rect) -> Self {
        let mut hash = Hasher::new(b"drip region");
        hash.0.update(self.hash);
        region.origin.iter().chain(&region.extent).for_each(|&v| hash.u64(v.into()));
        hash.finish(self.local)
    }

    /// The hash in lowercase hexadecimal, 64 characters.
    fn hex(self) -> String {
        self.hash.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Writes every field self-delimiting, by length or type tag, so distinct
/// field sequences hash distinct byte strings.
struct Hasher(Sha256);
impl Hasher {
    fn new(domain: &[u8]) -> Self {
        let mut hash = Self(Sha256::new());
        hash.bytes(domain);
        hash
    }
    fn finish(self, local: bool) -> Key {
        Key { hash: self.0.finalize().into(), local }
    }
    fn tag(&mut self, tag: u8) {
        self.0.update([tag]);
    }
    fn u64(&mut self, value: u64) {
        self.0.update(value.to_le_bytes());
    }
    fn bytes(&mut self, bytes: &[u8]) {
        self.u64(bytes.len() as u64);
        self.0.update(bytes);
    }
    /// Objects hash their fields in key order, so the encoding is canonical
    /// whatever order the JSON map keeps.
    fn json(&mut self, value: &Json) {
        match value {
            Json::Null => self.tag(0),
            Json::Bool(value) => self.tag(if *value { 2 } else { 1 }),
            Json::Number(number) => {
                self.tag(3);
                self.bytes(number.to_string().as_bytes());
            }
            Json::String(text) => {
                self.tag(4);
                self.bytes(text.as_bytes());
            }
            Json::Array(values) => {
                self.tag(5);
                self.u64(values.len() as u64);
                values.iter().for_each(|value| self.json(value));
            }
            Json::Object(fields) => {
                self.tag(6);
                self.u64(fields.len() as u64);
                let mut fields: Vec<_> = fields.iter().collect();
                fields.sort_unstable_by_key(|(name, _)| *name);
                for (name, value) in fields {
                    self.bytes(name.as_bytes());
                    self.json(value);
                }
            }
        }
    }
    fn file(&mut self, file: &FileStamp) {
        self.bytes(file.path.as_os_str().as_encoded_bytes());
        self.u64(file.len);
        // Times before the epoch take their own tag, apart from times after it.
        let modified = file.modified.map(|time| match time.duration_since(UNIX_EPOCH) {
            Ok(after) => (1, after),
            Err(before) => (2, before.duration()),
        });
        match modified {
            None => self.tag(0),
            Some((tag, offset)) => {
                self.tag(tag);
                self.u64(offset.as_secs());
                self.u64(offset.subsec_nanos().into());
            }
        }
    }
}

/// Meta expansions the store keeps. Each holds a few node definitions, and
/// an edit of a meta node's parameters adds one.
const EXPANSIONS: usize = 64;

/// Materialized outputs by key, within a byte budget per side, the foreign
/// jobs that produce some of them, and the expansions of meta nodes.
pub(crate) struct Store {
    entries: HashMap<Key, Entry>,
    budget: Budget,
    /// Advances on every insertion and hit, and orders entries by last use.
    clock: u64,
    evictions: usize,
    reads: usize,
    /// Foreign jobs by node key: running, finished but not yet taken, or
    /// failed. A failure stays, so demanding its key again repeats the error
    /// instead of running the node again. A new key, such as a new file
    /// stamp, runs it.
    jobs: HashMap<Key, Arc<Job>>,
    /// Expansions of meta nodes by the meta node's key, with the clock of
    /// their last use. Equal keys read equal input descriptions.
    expansions: HashMap<Key, (Arc<SubGraph>, u64)>,
    pub disk: Option<Arc<Disk>>,
    /// Called on a job's thread after the job finishes.
    pub wake: Option<Wake>,
}
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;
struct Entry {
    image: Image,
    durability: Durability,
    used: u64,
}
/// The outputs of one foreign node, set once by the job's thread.
#[derive(Default)]
struct Job {
    result: Mutex<Option<Result<Vec<Image>>>>,
    done: Condvar,
}

impl Store {
    pub fn new(budget: Budget) -> Self {
        Self {
            entries: HashMap::new(),
            budget,
            clock: 0,
            evictions: 0,
            reads: 0,
            jobs: HashMap::new(),
            expansions: HashMap::new(),
            disk: None,
            wake: None,
        }
    }

    /// The image stored under `key`, marked as used now.
    pub fn get(&mut self, key: Key) -> Option<Image> {
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.used = self.clock;
        Some(entry.image.clone())
    }

    /// The image the disk tier holds under `key`, read with `codec` into a
    /// host representation and kept in memory as a `High` entry. `None` when
    /// there is no disk tier, `key` is local, or the entry is missing or
    /// unreadable.
    pub fn read(&mut self, key: Key, codec: &Codec) -> Option<Image> {
        let image = self.disk.as_ref().filter(|_| !key.local)?.read(key, codec)?;
        self.reads += 1;
        self.insert(key, image.clone(), Durability::High);
        Some(image)
    }

    /// Keep `image`, which must be materialized, under `key`, then trim. An
    /// entry already under `key` stays and is marked as used: its samples are
    /// equal, and entries are never replaced.
    pub fn insert(&mut self, key: Key, image: Image, durability: Durability) {
        debug_assert!(image.footprint().is_some(), "the store keeps materialized images");
        self.clock += 1;
        let used = self.clock;
        self.entries.entry(key).or_insert(Entry { image, durability, used }).used = used;
        self.trim();
    }

    /// Drop representations until each side is within the budget: device
    /// first, then host, each time from the entry with the lowest durability
    /// and, among those, the oldest use. An entry that loses its last
    /// representation leaves the store. Representations change after
    /// insertion, when a consumer uploads or downloads an entry, so evaluation
    /// trims again when it ends.
    pub fn trim(&mut self) {
        for (side, limit) in [(Side::Device, self.budget.device), (Side::Host, self.budget.host)] {
            let bytes = |entry: &Entry| {
                entry.image.footprint().expect("stored images are materialized").on(side)
            };
            let mut total: u64 = self.entries.values().map(bytes).sum();
            while total > limit {
                let (&key, entry) = self
                    .entries
                    .iter_mut()
                    .filter(|(_, entry)| bytes(entry) > 0)
                    .min_by_key(|(_, entry)| (entry.durability, entry.used))
                    .expect("a side over budget has an entry holding bytes");
                total -= bytes(entry);
                self.evictions += 1;
                match entry.image.without(side) {
                    Some(rest) => entry.image = rest,
                    None => drop(self.entries.remove(&key)),
                }
            }
        }
    }

    /// The expansion of the meta node keyed `key`: the one kept, or a new
    /// one from `expand`, kept unless it failed. The least recently used of
    /// more than [`EXPANSIONS`] leaves.
    pub fn expansion(
        &mut self,
        key: Key,
        expand: impl FnOnce() -> Result<SubGraph>,
    ) -> Result<Arc<SubGraph>> {
        self.clock += 1;
        if let Some((graph, used)) = self.expansions.get_mut(&key) {
            *used = self.clock;
            return Ok(graph.clone());
        }
        let graph = Arc::new(expand()?);
        self.expansions.insert(key, (graph.clone(), self.clock));
        if self.expansions.len() > EXPANSIONS {
            let oldest = self.expansions.iter().min_by_key(|(_, (_, used))| *used);
            let oldest = *oldest.expect("the map is not empty").0;
            self.expansions.remove(&oldest);
        }
        Ok(graph)
    }

    /// Representations dropped since the store was made.
    pub fn evictions(&self) -> usize {
        self.evictions
    }

    /// Move the outputs of jobs that succeeded since the last call into
    /// entries, so a job no demand takes any more falls under the budget.
    pub fn collect(&mut self) {
        let finished: Vec<_> = self
            .jobs
            .iter()
            .filter_map(|(key, job)| {
                let result = job.result.lock().unwrap_or_else(PoisonError::into_inner);
                Some((*key, result.as_ref()?.as_ref().ok()?.clone()))
            })
            .collect();
        for (key, images) in finished {
            self.jobs.remove(&key);
            for (index, image) in images.into_iter().enumerate() {
                self.insert(key.output(index), image, Durability::High);
            }
        }
    }

    /// Values read from the disk tier since the store was made.
    pub fn reads(&self) -> usize {
        self.reads
    }

    /// The outputs of the foreign node keyed `key`. A job under `key` that
    /// is running or finished answers. Otherwise a new job runs `compute` on
    /// a thread of its own, writes the outputs to the disk tier when `cache`
    /// asks for it, then calls the wake callback. With `wait` the call blocks
    /// until the job ends, and without it a running job answers `None`. A
    /// successful job leaves the store when it answers, and the caller offers
    /// its outputs as entries.
    pub fn foreign(
        &mut self,
        key: Key,
        cache: Cache,
        wait: bool,
        compute: impl FnOnce() -> Result<Vec<Image>> + Send + 'static,
    ) -> Option<Result<Vec<Image>>> {
        let job = match self.jobs.get(&key) {
            Some(job) => job.clone(),
            None => {
                let job = self.spawn(key, cache, compute);
                self.jobs.insert(key, job.clone());
                job
            }
        };
        let mut result = job.result.lock().unwrap_or_else(PoisonError::into_inner);
        while wait && result.is_none() {
            result = job.done.wait(result).unwrap_or_else(PoisonError::into_inner);
        }
        let outcome = result.clone()?;
        drop(result);
        if outcome.is_ok() {
            self.jobs.remove(&key);
        }
        Some(outcome)
    }

    fn spawn(
        &self,
        key: Key,
        cache: Cache,
        compute: impl FnOnce() -> Result<Vec<Image>> + Send + 'static,
    ) -> Arc<Job> {
        let job = Arc::new(Job::default());
        let (finished, wake) = (job.clone(), self.wake.clone());
        let disk = match cache {
            Cache::Disk(codecs) => self.disk.clone().filter(|_| !key.local).zip(Some(codecs)),
            Cache::Memory => None,
        };
        let run = move || {
            // A panic fails the job, so a caller waiting for it returns.
            let result = panic::catch_unwind(AssertUnwindSafe(compute))
                .unwrap_or_else(|_| Err(Error::Runtime("the node panicked".into())));
            // Written before waking, so another evaluator demanding the key
            // after the wake finds the files.
            if let (Ok(images), Some((disk, codecs))) = (&result, disk) {
                disk.write(key, codecs, images);
            }
            finished.finish(result);
            if let Some(wake) = wake {
                wake();
            }
        };
        if let Err(error) = thread::Builder::new().name("drip-foreign".into()).spawn(run) {
            job.finish(Err(Error::Runtime(format!("cannot start a thread: {error}"))));
        }
        job
    }
}

impl Job {
    fn finish(&self, result: Result<Vec<Image>>) {
        *self.result.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        self.done.notify_all();
    }
}

/// The store's disk tier: a directory of files, one per output, named by the
/// output's key in hexadecimal. A file holds a format line, the payload's
/// name, then the payload's [`Persist`](crate::payload::Persist) stream.
/// Files are written once, under a temporary name renamed into place, and
/// may be deleted at any time. A file that fails to read is deleted and
/// counts as missing. Over `budget` bytes, the oldest used files go first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Created on the first write. The store deletes files in it whose names
    /// start with a key.
    pub directory: PathBuf,
    pub budget: u64,
}
const FORMAT: &[u8] = b"drip store 1\n";
impl Disk {
    /// `directory` with a 16 GiB budget.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self { directory: directory.into(), budget: 16 << 30 }
    }

    /// `drip/store` in the platform's cache directory: `$XDG_CACHE_HOME`, or
    /// `~/.cache` when it is unset, on Linux. `None` where the platform has
    /// none.
    pub fn platform() -> Option<Self> {
        dirs::cache_dir().map(|cache| Self::new(cache.join("drip").join("store")))
    }

    fn path(&self, key: Key) -> PathBuf {
        self.directory.join(key.hex())
    }

    fn read(&self, key: Key, codec: &Codec) -> Option<Image> {
        let path = self.path(key);
        let file = File::open(&path).ok()?;
        match decode(&mut BufReader::new(&file), codec) {
            Ok(image) => {
                // The modification time records the last use for trimming.
                // Setting it needs write access to the file on Windows, which
                // the read handle lacks, so a second handle touches it.
                let touched = fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|file| file.set_modified(SystemTime::now()));
                if let Err(error) = touched {
                    log::debug!("cannot touch stored {}: {error}", path.display());
                }
                Some(image)
            }
            Err(error) => {
                log::warn!("discarding stored {}: {error}", path.display());
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    /// Write each image under its output key, then trim. A failure leaves
    /// that output unstored and is logged.
    fn write(&self, key: Key, codecs: &[Codec], images: &[Image]) {
        for (index, (codec, image)) in codecs.iter().zip(images).enumerate() {
            let path = self.path(key.output(index));
            if let Err(error) = self.create(&path, codec, image) {
                log::warn!("cannot store {}: {error}", path.display());
            }
        }
        self.trim();
    }

    fn create(&self, path: &std::path::Path, codec: &Codec, image: &Image) -> Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        fs::create_dir_all(&self.directory).map_err(io_error)?;
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let partial = path.with_extension(format!("{}-{unique}.partial", std::process::id()));
        let result = File::create(&partial).map_err(io_error).and_then(|file| {
            let mut out = BufWriter::new(file);
            out.write_all(FORMAT).map_err(io_error)?;
            write_json(&codec.payload, &mut out).map_err(io_error)?;
            (codec.write)(image, &mut out)?;
            out.into_inner().map_err(|e| io_error(e.into_error()))?;
            fs::rename(&partial, path).map_err(io_error)
        });
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result
    }

    /// Delete the oldest used files until the directory's entries fit the
    /// budget.
    fn trim(&self) {
        let Ok(directory) = fs::read_dir(&self.directory) else { return };
        let mut files: Vec<_> = directory
            .flatten()
            .filter(|file| {
                let name = file.file_name();
                let name = name.as_encoded_bytes();
                name.len() >= 64 && name[..64].iter().all(u8::is_ascii_hexdigit)
            })
            .filter_map(|file| {
                let metadata = file.metadata().ok().filter(|m| m.is_file())?;
                Some((metadata.modified().unwrap_or(UNIX_EPOCH), metadata.len(), file.path()))
            })
            .collect();
        files.sort_unstable();
        let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
        for (_, len, path) in files {
            if total <= self.budget {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
}

/// Read one file's contents, which must end where the payload's stream does.
fn decode(input: &mut dyn Read, codec: &Codec) -> Result<Image> {
    let mut format = [0; FORMAT.len()];
    input.read_exact(&mut format).map_err(io_error)?;
    let payload: String = read_json(input).map_err(io_error)?;
    if format != FORMAT || payload != codec.payload {
        return Err(Error::Runtime(format!("not a stored {}", codec.payload)));
    }
    let image = (codec.read)(input)?;
    match input.read(&mut [0]).map_err(io_error)? {
        0 => Ok(image),
        _ => Err(Error::Runtime("trailing bytes".into())),
    }
}

fn io_error(error: io::Error) -> Error {
    Error::Runtime(error.to_string())
}
