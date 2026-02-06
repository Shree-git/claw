use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revision {
    pub change_id: Option<crate::id::ChangeId>,
    pub parents: Vec<ObjectId>,
    pub patches: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_base: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capsule_id: Option<ObjectId>,
    pub author: String,
    pub created_at_ms: u64,
    pub summary: String,
    #[serde(default)]
    pub policy_evidence: Vec<String>,
}
