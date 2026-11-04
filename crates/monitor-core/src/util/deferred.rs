//! Blocking reads moved off the sampler thread: slow EC, NVMe or firmware
//! backed attributes are read by a worker that wakes only when asked, so a
//! tick never waits on hardware and no thread wakes between ticks.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

struct Slot<Output> {
    output: Output,
    fresh: bool,
}

/// A worker thread owning `Source`. [`DeferredReader::request`] asks for one
/// read; [`DeferredReader::take_fresh`] hands over the newest result once.
/// The thread ends when the reader is dropped.
pub struct DeferredReader<Output> {
    requests: SyncSender<()>,
    slot: Arc<Mutex<Slot<Output>>>,
}

impl<Output> std::fmt::Debug for DeferredReader<Output> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeferredReader")
            .finish_non_exhaustive()
    }
}

impl<Output: Default + Send + 'static> DeferredReader<Output> {
    /// `read` must overwrite its output completely: buffers are swapped with
    /// the published slot, never reallocated. `None` when no thread starts.
    pub fn spawn<Source: Send + 'static>(
        name: &str,
        mut source: Source,
        mut read: impl FnMut(&mut Source, &mut Output) + Send + 'static,
    ) -> Option<Self> {
        let (requests, pending) = mpsc::sync_channel(1);
        let slot = Arc::new(Mutex::new(Slot {
            output: Output::default(),
            fresh: false,
        }));
        let published = Arc::clone(&slot);
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || serve(&pending, &published, |output| read(&mut source, output)))
            .ok()?;
        Some(Self { requests, slot })
    }

    /// Queues a read unless one is already queued; never blocks.
    pub fn request(&self) {
        let _ = self.requests.try_send(());
    }

    /// Calls `apply` with a result not taken before. Returns `false`, without
    /// waiting, when there is none or the worker is publishing right now.
    pub fn take_fresh(&self, apply: impl FnOnce(&Output)) -> bool {
        let Ok(mut slot) = self.slot.try_lock() else {
            return false;
        };
        if !std::mem::take(&mut slot.fresh) {
            return false;
        }
        apply(&slot.output);
        true
    }
}

fn serve<Output: Default>(
    pending: &Receiver<()>,
    slot: &Mutex<Slot<Output>>,
    mut read: impl FnMut(&mut Output),
) {
    let mut output = Output::default();
    while pending.recv().is_ok() {
        read(&mut output);
        let Ok(mut published) = slot.lock() else {
            return;
        };
        std::mem::swap(&mut published.output, &mut output);
        published.fresh = true;
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn hands_each_result_over_once() {
        let Some(reader) = DeferredReader::spawn("deferred-test", 0_u32, |count, out: &mut u32| {
            *count += 1;
            *out = *count;
        }) else {
            return;
        };
        assert!(!reader.take_fresh(|_| {}));
        reader.request();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = None;
        while seen.is_none() && Instant::now() < deadline {
            reader.take_fresh(|value| seen = Some(*value));
            std::thread::yield_now();
        }
        assert_eq!(seen, Some(1));
        assert!(!reader.take_fresh(|_| {}));
    }
}
