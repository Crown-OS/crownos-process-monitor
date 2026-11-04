use std::sync::{Arc, Mutex};

/// The most recent value a background worker published. The sampler reads it
/// with `try_lock`, so a busy worker never stalls a tick.
#[derive(Debug)]
pub struct Latest<T> {
    slot: Mutex<Arc<T>>,
}

impl<T: Default> Default for Latest<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(Arc::new(T::default())),
        }
    }
}

impl<T> Latest<T> {
    pub fn publish(&self, value: T) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Arc::new(value);
        }
    }

    /// The published value, or `None` while the worker holds the lock.
    #[must_use]
    pub fn try_get(&self) -> Option<Arc<T>> {
        self.slot.try_lock().ok().map(|slot| Arc::clone(&slot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_new_values() {
        let latest = Latest::<u32>::default();
        assert_eq!(latest.try_get().as_deref(), Some(&0));
        latest.publish(7);
        assert_eq!(latest.try_get().as_deref(), Some(&7));
    }
}
