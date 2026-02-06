use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefLog {
    pub ref_name: String,
    pub entries: Vec<RefLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefLogEntry {
    pub old_target: Option<ObjectId>,
    pub new_target: ObjectId,
    pub author: String,
    pub message: String,
    pub timestamp: u64,
}
