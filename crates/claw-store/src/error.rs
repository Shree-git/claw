use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not a claw repository: {0}")]
    NotARepository(PathBuf),
    #[error("object not found: {0}")]
    ObjectNotFound(claw_core::id::ObjectId),
    #[error("ref not found: {0}")]
    RefNotFound(String),
    #[error("lock contention on {0}")]
    LockContention(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("core error: {0}")]
    Core(#[from] claw_core::CoreError),
    #[error("config error: {0}")]
    Config(String),
    #[error("index error: {0}")]
    Index(String),
    #[error("ref CAS conflict: expected {expected}, actual {actual}")]
    RefCasConflict { expected: String, actual: String },
}
