use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsulePublic {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_fingerprint: Option<String>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleSignature {
    pub signer_id: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub revision_id: ObjectId,
    pub public_fields: CapsulePublic,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_private: Option<Vec<u8>>,
    #[serde(default)]
    pub encryption: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default)]
    pub signatures: Vec<CapsuleSignature>,
}
