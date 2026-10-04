//! Files that nodes read (DESIGN E2, E7). Each file has a revision that only an
//! explicit reload bumps; results depending on a file include its revision in
//! their stamps. What is loaded from a file is shared until the next reload, so
//! expensive decodes survive changes that don't affect them, such as scale.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Loaded = HashMap<(PathBuf, TypeId), Arc<dyn Any + Send + Sync>>;

#[derive(Default)]
pub struct Resources {
    revisions: HashMap<PathBuf, u64>,
    loaded: Mutex<Loaded>,
}

impl Resources {
    pub fn revision(&self, path: &Path) -> u64 {
        self.revisions.get(path).copied().unwrap_or(0)
    }

    pub fn reload(&mut self, path: &Path) {
        *self.revisions.entry(path.into()).or_default() += 1;
        self.loaded.get_mut().expect("not poisoned").retain(|(loaded, _), _| loaded != path);
    }

    /// What `load` makes of `path`, computed once per revision and type.
    /// Failures are not kept, so they are retried.
    pub fn load<T: Any + Send + Sync>(
        &self,
        path: &Path,
        load: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        let key = (path.to_path_buf(), TypeId::of::<T>());
        if let Some(value) = self.loaded.lock().expect("not poisoned").get(&key) {
            return Ok(value.clone().downcast().expect("keyed by type"));
        }
        let value = Arc::new(load(path)?);
        self.loaded.lock().expect("not poisoned").insert(key, value.clone());
        Ok(value)
    }
}
