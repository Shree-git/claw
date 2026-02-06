use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub tree_root: ObjectId,
    pub revision_id: ObjectId,
    pub created_at_ms: u64,
}
