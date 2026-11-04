use std::time::Duration;

/// Waits `duration` on the task runtime. The timer is created when the task
/// first runs, so this is safe to hand to `cx.spawn` from the UI thread,
/// which has no Tokio context of its own.
pub async fn after(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_be_created_outside_a_runtime() {
        let pending = after(Duration::from_millis(1));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build();
        let Ok(runtime) = runtime else {
            panic!("a current-thread runtime should build");
        };
        runtime.block_on(pending);
    }
}
