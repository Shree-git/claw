use thiserror::Error;

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("codec not found: {0}")]
    CodecNotFound(String),
    #[error("apply failed: {0}")]
    ApplyFailed(String),
    #[error("invert failed: {0}")]
    InvertFailed(String),
    #[error("commute failed: patches overlap")]
    CommuteFailed,
    #[error("merge3 failed: {0}")]
    Merge3Failed(String),
    #[error("address resolution failed: {0}")]
    AddressResolutionFailed(String),
    #[error("invalid json: {0}")]
    InvalidJson(String),
}
