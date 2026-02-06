use serde::{Deserialize, Serialize};

use crate::id::ObjectId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchOp {
    pub address: String,
    pub op_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_data: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_data: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_hash: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub target_path: String,
    pub codec_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_object: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_object: Option<ObjectId>,
    pub ops: Vec<PatchOp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_payload: Option<Vec<u8>>,
}
