use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unsupported COF version: {0}")]
    UnsupportedVersion(u8),
    #[error("unknown type tag: {0}")]
    UnknownTypeTag(u8),
    #[error("CRC32 mismatch: expected {expected:#010x}, got {actual:#010x}")]
    Crc32Mismatch { expected: u32, actual: u32 },
    #[error("decompression error: {0}")]
    Decompression(String),
    #[error("compression error: {0}")]
    Compression(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("deserialization error: {0}")]
    Deserialization(String),
    #[error("invalid object ID: {0}")]
    InvalidObjectId(String),
    #[error("payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: usize, max: usize },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
