use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStatus {
    Open,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub base_revision: Option<ObjectId>,
    pub left_revision: ObjectId,
    pub right_revision: ObjectId,
    pub file_path: String,
    pub codec_id: String,
    pub left_patch_ids: Vec<ObjectId>,
    pub right_patch_ids: Vec<ObjectId>,
    #[serde(default)]
    pub resolution_patch_ids: Vec<ObjectId>,
    pub status: ConflictStatus,
    pub created_at_ms: u64,
}
