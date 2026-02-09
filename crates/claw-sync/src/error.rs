use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("negotiation failed: {0}")]
    NegotiationFailed(String),
    #[error("transfer failed: {0}")]
    TransferFailed(String),
    #[error("store error: {0}")]
    Store(#[from] claw_store::StoreError),
    #[error("core error: {0}")]
    Core(#[from] claw_core::CoreError),
    #[error("grpc error: {0}")]
    Grpc(#[from] tonic::Status),
    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
