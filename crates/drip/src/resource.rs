//! Files loaded once per resource store, shared by preview and export.
//! Replacing the store invalidates its contents without affecting existing readers.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Loaded = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;
type Files = HashMap<PathBuf, Arc<Mutex<Loaded>>>;

#[derive(Clone, Default)]
pub struct Resources {
    files: Arc<Mutex<Files>>,
}

impl Resources {
    /// Computes a value once per path and type. Failures are retried.
    pub fn load<T: Any + Send + Sync>(
        &self,
        path: &Path,
        load: impl FnOnce(&Path) -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        let file = self.files.lock().expect("not poisoned").entry(path.into()).or_default().clone();
        // Serialize the first load, including concurrent preview/export readers.
        let mut loaded = file.lock().expect("not poisoned");
        let key = TypeId::of::<T>();
        if let Some(value) = loaded.get(&key) {
            log::trace!(
                "resource cache=hit path={} type={}",
                path.display(),
                std::any::type_name::<T>()
            );
            return Ok(value.clone().downcast().expect("keyed by type"));
        }
        let start = std::time::Instant::now();
        let result = load(path);
        log::debug!(
            "resource path={} type={} outcome={} elapsed={:.1?}",
            path.display(),
            std::any::type_name::<T>(),
            if result.is_ok() { "ok" } else { "failed" },
            start.elapsed()
        );
        let value = Arc::new(result?);
        loaded.insert(key, value.clone());
        Ok(value)
    }
}
