use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeState {
    pub merge: MergeInfo,
    #[serde(default)]
    pub conflicts: Vec<ConflictEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeInfo {
    pub left_ref: String,
    pub right_ref: String,
    pub left_revision: String,
    pub right_revision: String,
    pub base_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEntry {
    pub file_path: String,
    pub conflict_id: String,
    pub codec_id: String,
}

const MERGE_STATE_FILE: &str = "MERGE_STATE.toml";

pub fn write_to(claw_dir: &Path, state: &MergeState) -> anyhow::Result<()> {
    let content = toml::to_string_pretty(state)?;
    std::fs::write(claw_dir.join(MERGE_STATE_FILE), content)?;
    Ok(())
}

pub fn read_from(claw_dir: &Path) -> anyhow::Result<MergeState> {
    let content = std::fs::read_to_string(claw_dir.join(MERGE_STATE_FILE))?;
    let state: MergeState = toml::from_str(&content)?;
    Ok(state)
}

pub fn exists(claw_dir: &Path) -> bool {
    claw_dir.join(MERGE_STATE_FILE).exists()
}

pub fn remove(claw_dir: &Path) -> anyhow::Result<()> {
    let path = claw_dir.join(MERGE_STATE_FILE);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
