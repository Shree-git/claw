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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<ConflictRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRegion {
    pub side: String,
    pub patch_id: String,
    pub op_type: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_bytes: Option<usize>,
}

const MERGE_STATE_FILE: &str = "MERGE_STATE.toml";

pub fn write_to(claw_dir: &Path, state: &MergeState) -> anyhow::Result<()> {
    let content = toml::to_string_pretty(state)?;
    write_atomic(&claw_dir.join(MERGE_STATE_FILE), content.as_bytes())
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

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    temp.persist(path).map_err(|err| err.error)?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_state_roundtrips_through_atomic_writer() {
        let tmp = tempfile::tempdir().unwrap();
        let state = MergeState {
            merge: MergeInfo {
                left_ref: "heads/main".to_string(),
                right_ref: "heads/feature".to_string(),
                left_revision: "left".to_string(),
                right_revision: "right".to_string(),
                base_revision: "base".to_string(),
            },
            conflicts: vec![ConflictEntry {
                file_path: "src/lib.rs".to_string(),
                conflict_id: "conflict-1".to_string(),
                codec_id: "text".to_string(),
                reason: Some("overlap".to_string()),
                regions: Vec::new(),
            }],
        };

        write_to(tmp.path(), &state).unwrap();
        let read_back = read_from(tmp.path()).unwrap();

        assert_eq!(read_back.merge.left_ref, "heads/main");
        assert_eq!(read_back.conflicts.len(), 1);
        assert_eq!(read_back.conflicts[0].conflict_id, "conflict-1");
    }
}
