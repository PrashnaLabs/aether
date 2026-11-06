use std::sync::{Mutex, MutexGuard};

/// Lock that recovers a poisoned mutex instead of panicking.
pub trait MutexExt<T> {
    fn guard(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn guard(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}
