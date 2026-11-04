use tokio::sync::{broadcast, oneshot};

/// Waits for `reply` on a short-lived thread and broadcasts what `outcome`
/// makes of it (`None` when the replier went away), so the sampler never
/// waits on a D-Bus answer or an authentication dialog.
pub(crate) fn forward<Reply, Outcome>(
    thread_name: &str,
    reply: oneshot::Receiver<Reply>,
    outcomes: &broadcast::Sender<Outcome>,
    outcome: impl FnOnce(Option<Reply>) -> Outcome + Send + 'static,
) where
    Reply: Send + 'static,
    Outcome: Send + 'static,
{
    let outcomes = outcomes.clone();
    let waiting = std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let _ = outcomes.send(outcome(reply.blocking_recv().ok()));
        });
    drop(waiting);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn broadcasts_the_answer_and_a_dropped_replier() {
        let (outcomes, mut received) = broadcast::channel(4);
        let (answer, reply) = oneshot::channel();
        forward("test-reply", reply, &outcomes, |reply| reply.unwrap_or(0));
        let _ = answer.send(7);
        let (dropped, reply) = oneshot::channel::<u8>();
        drop(dropped);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build();
        let Ok(runtime) = runtime else { return };
        let mut seen = runtime.block_on(async {
            let first = tokio::time::timeout(Duration::from_secs(5), received.recv()).await;
            forward("test-reply", reply, &outcomes, |reply| reply.unwrap_or(0));
            let second = tokio::time::timeout(Duration::from_secs(5), received.recv()).await;
            [first, second].map(|answer| answer.ok().and_then(Result::ok))
        });
        seen.sort();
        assert_eq!(seen, [Some(0), Some(7)]);
    }
}
