use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, carrying on with the data when a panicking holder
/// poisoned it: every guarded value here stays consistent between calls.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
