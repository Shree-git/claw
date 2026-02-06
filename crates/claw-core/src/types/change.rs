use serde::{Deserialize, Serialize};

use crate::id::{ChangeId, IntentId, ObjectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeStatus {
    Open,
    Ready,
    Integrated,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub id: ChangeId,
    pub intent_id: IntentId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_revision: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workstream_id: Option<String>,
    pub status: ChangeStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
