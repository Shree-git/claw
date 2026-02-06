use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("policy violation: {0}")]
    Violation(String),
    #[error("missing required check: {0}")]
    MissingCheck(String),
    #[error("visibility denied")]
    VisibilityDenied,
}
