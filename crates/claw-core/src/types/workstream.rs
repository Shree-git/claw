use serde::{Deserialize, Serialize};

use crate::id::ChangeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstream {
    pub workstream_id: String,
    #[serde(default)]
    pub change_stack: Vec<ChangeId>,
}
