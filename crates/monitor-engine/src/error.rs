use monitor_history::HistoryError;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("cannot start the sampler thread: {0}")]
    Spawn(#[source] std::io::Error),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("history is not being recorded")]
    NoHistory,
    #[error("the engine has stopped")]
    Stopped,
}
