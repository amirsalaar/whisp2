//! Test-only helpers shared across modules.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

// `app_support_dir()` resolves off the process-wide `HOME`, so any test that
// points it at a tempdir must hold this single lock. One lock for the whole
// test binary — per-module locks don't coordinate and let HOME-mutating tests
// in different modules race each other.
static HOME_LOCK: Mutex<()> = Mutex::new(());

/// Sets `HOME` to `new_home` for the guard's lifetime, serialized against every
/// other `HomeGuard` in the test binary, and restores the previous value on
/// drop.
pub(crate) struct HomeGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<String>,
}

impl HomeGuard {
    pub(crate) fn new(new_home: &Path) -> Self {
        let lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("HOME").ok();
        std::env::set_var("HOME", new_home);
        Self {
            _lock: lock,
            previous,
        }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
