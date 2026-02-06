use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitExportError {
    #[error("store error: {0}")]
    Store(#[from] claw_store::StoreError),
    #[error("core error: {0}")]
    Core(#[from] claw_core::CoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("object not found: {0}")]
    ObjectNotFound(String),
    #[error("invalid object type for git export: {0}")]
    InvalidType(String),
}
