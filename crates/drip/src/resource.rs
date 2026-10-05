//! Files that nodes read (DESIGN E2, E7). Each file has a revision that only an
//! explicit reload bumps; results depending on a file include its revision in
//! their stamps. What is loaded from a file is shared until the next reload, so
//! expensive decodes survive changes that don't affect them, such as scale.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Loaded = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

#[derive(Clone, Default)]
struct File {
    revision: u64,
    loaded: Arc<Mutex<Loaded>>,
}

#[derive(Default)]
pub struct Resources {
    files: Mutex<HashMap<PathBuf, File>>,
}

impl Resources {
    pub fn revision(&self, path: &Path) -> u64 {
        self.files.lock().expect("not poisoned").get(path).map_or(0, |file| file.revision)
    }

    pub fn reload(&mut self, path: &Path) {
        let file = self.files.get_mut().expect("not poisoned").entry(path.into()).or_default();
        *file = File { revision: file.revision + 1, ..File::default() };
    }

    /// Shares the current revisions, including files not yet loaded. Reloading
    /// either store afterwards leaves the other's revisions and values intact.
    pub fn snapshot<'a>(&self, paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut files = self.files.lock().expect("not poisoned");
        for path in paths {
            files.entry(path.into()).or_default();
        }
        Self { files: Mutex::new(files.clone()) }
    }

    /// What `load` makes of `path`, computed once per revision and type.
    /// Failures are not kept, so they are retried.
    pub fn load<T: Any + Send + Sync>(
        &self,
        path: &Path,
        load: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        let file = self.files.lock().expect("not poisoned").entry(path.into()).or_default().clone();
        // Serialize loads of this revision, also across export snapshots.
        let mut loaded = file.loaded.lock().expect("not poisoned");
        let key = TypeId::of::<T>();
        if let Some(value) = loaded.get(&key) {
            return Ok(value.clone().downcast().expect("keyed by type"));
        }
        let value = Arc::new(load(path)?);
        loaded.insert(key, value.clone());
        Ok(value)
    }
}
