use std::sync::Arc;
use std::time::Duration;

use crownui::prelude::{Cx, Signal};
use monitor_core::{ProcessKey, SourceError};
use monitor_engine::system::inspect::{ProcessDetails, inspect};

use crate::app::EngineLink;

/// How often the open drawer re-reads its process.
pub const REFRESH: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default)]
pub enum Loaded {
    #[default]
    Loading,
    Ready(Arc<ProcessDetails>),
    Ended,
    Failed(String),
}

/// The latest details and a revision that changes with every answer, so sections rebuild
/// only when there is something new.
#[derive(Debug, Clone, Default)]
pub struct InspectorData {
    pub revision: u64,
    pub loaded: Loaded,
}

impl InspectorData {
    pub fn details(&self) -> Option<&Arc<ProcessDetails>> {
        match &self.loaded {
            Loaded::Ready(details) => Some(details),
            Loaded::Loading | Loaded::Ended | Loaded::Failed(_) => None,
        }
    }
}

/// Reads `key` now and every [`REFRESH`] while the calling scope lives: on the engine's
/// sampler thread, which reuses its desktop index, or on a blocking task without an engine.
/// Mount it under a `keyed` on the target, so a new target cancels the old loop.
pub fn follow(cx: &mut Cx, key: ProcessKey, data: Signal<InspectorData>) {
    let engine = EngineLink::find(cx).map(|link| link.inspect(key));
    let work = async move {
        match engine {
            Some(answer) => answer.await,
            None => tokio::task::spawn_blocking(move || inspect(key))
                .await
                .unwrap_or(Err(SourceError::Unavailable("the inspector"))),
        }
    };
    cx.spawn(work, move |cx, answer| {
        let loaded = match answer {
            Ok(details) => Loaded::Ready(Arc::new(details)),
            Err(error) if error.is_gone() => Loaded::Ended,
            Err(error) => Loaded::Failed(error.to_string()),
        };
        let keep_going = matches!(loaded, Loaded::Ready(_));
        data.update(cx, |data| {
            data.revision += 1;
            data.loaded = loaded;
        });
        if keep_going {
            cx.spawn(crate::util::delay::after(REFRESH), move |cx, ()| {
                follow(cx, key, data);
            });
        }
    });
}
