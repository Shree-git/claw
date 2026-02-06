use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("keypair generation failed: {0}")]
    KeyGeneration(String),
    #[error("signing failed: {0}")]
    SigningFailed(String),
    #[error("verification failed: {0}")]
    VerificationFailed(String),
    #[error("encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("core error: {0}")]
    Core(#[from] claw_core::CoreError),
}
