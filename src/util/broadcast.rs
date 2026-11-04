use crownui::prelude::Cx;
use tokio::sync::broadcast::{Receiver, error::RecvError};

/// Calls `on_message` on the UI thread for every message of `receiver` while the calling
/// component lives. A lagging receiver skips what it missed; a closed one stops.
pub fn follow_broadcast<T, F>(cx: &mut Cx, mut receiver: Receiver<T>, mut on_message: F)
where
    T: Clone + Send + 'static,
    F: FnMut(&mut Cx, T) + 'static,
{
    let next = async move {
        let message = receiver.recv().await;
        (receiver, message)
    };
    cx.spawn(next, move |cx, (receiver, message)| match message {
        Ok(message) => {
            on_message(cx, message);
            follow_broadcast(cx, receiver, on_message);
        }
        Err(RecvError::Lagged(_)) => follow_broadcast(cx, receiver, on_message),
        Err(RecvError::Closed) => {}
    });
}
