#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("history database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("history file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("history schema version {found} is newer than the supported {supported}")]
    UnsupportedSchema { found: i32, supported: i32 },
    #[error("no home directory to place history files in")]
    NoHomeDirectory,
    #[error("the history writer thread has stopped")]
    WriterStopped,
}

pub type HistoryResult<T> = Result<T, HistoryError>;
