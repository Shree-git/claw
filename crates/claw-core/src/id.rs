use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};
use std::fmt;
use ulid::Ulid;

use crate::CoreError;

const OBJECT_ID_PREFIX: &str = "clw_";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId([u8; 32]);

impl ObjectId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, CoreError> {
        let bytes = hex::decode(s).map_err(|e| CoreError::InvalidObjectId(e.to_string()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CoreError::InvalidObjectId("expected 32 bytes".into()))?;
        Ok(Self(arr))
    }

    pub fn from_display(s: &str) -> Result<Self, CoreError> {
        let encoded = s.strip_prefix(OBJECT_ID_PREFIX).ok_or_else(|| {
            CoreError::InvalidObjectId(format!("missing prefix '{OBJECT_ID_PREFIX}'"))
        })?;
        let upper = encoded.to_uppercase();
        let bytes = BASE32_NOPAD
            .decode(upper.as_bytes())
            .map_err(|e| CoreError::InvalidObjectId(e.to_string()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CoreError::InvalidObjectId("expected 32 bytes".into()))?;
        Ok(Self(arr))
    }

    /// First 2 hex chars used for loose object directory sharding
    pub fn shard_prefix(&self) -> String {
        hex::encode(&self.0[..1])
    }

    /// Remaining hex chars for the loose object filename
    pub fn shard_suffix(&self) -> String {
        hex::encode(&self.0[1..])
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoded = BASE32_NOPAD.encode(&self.0).to_lowercase();
        write!(f, "{OBJECT_ID_PREFIX}{encoded}")
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IntentId(Ulid);

impl IntentId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Ulid::from_bytes(bytes))
    }

    pub fn as_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }

    pub fn from_string(s: &str) -> Result<Self, CoreError> {
        let ulid = Ulid::from_string(s).map_err(|e| CoreError::InvalidObjectId(e.to_string()))?;
        Ok(Self(ulid))
    }
}

impl Default for IntentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for IntentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for IntentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IntentId({})", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChangeId(Ulid);

impl ChangeId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Ulid::from_bytes(bytes))
    }

    pub fn as_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }

    pub fn from_string(s: &str) -> Result<Self, CoreError> {
        let ulid = Ulid::from_string(s).map_err(|e| CoreError::InvalidObjectId(e.to_string()))?;
        Ok(Self(ulid))
    }
}

impl Default for ChangeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChangeId({})", self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConflictId(Ulid);

impl ConflictId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Ulid::from_bytes(bytes))
    }

    pub fn as_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }
}

impl Default for ConflictId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConflictId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for ConflictId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConflictId({})", self.0)
    }
}
