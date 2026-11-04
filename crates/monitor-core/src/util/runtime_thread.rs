use std::future::Future;
use std::io;

/// Runs `task` on a dedicated thread named `name` that owns a current-thread
/// tokio runtime, so a slow bus never stalls the sampler. The thread ends
/// when the task returns; it also ends at once if the runtime cannot start.
pub fn spawn_runtime_thread<Task, Work>(name: &str, task: Task) -> io::Result<()>
where
    Task: FnOnce() -> Work + Send + 'static,
    Work: Future<Output = ()>,
{
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(task());
        })
        .map(drop)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn runs_the_task_on_its_own_thread() {
        let (done, finished) = mpsc::channel();
        let spawned = spawn_runtime_thread("runtime-thread-test", move || async move {
            let _ = done.send(std::thread::current().name().map(str::to_owned));
        });
        assert!(spawned.is_ok());
        assert_eq!(
            finished
                .recv_timeout(Duration::from_secs(5))
                .ok()
                .flatten()
                .as_deref(),
            Some("runtime-thread-test")
        );
    }
}
