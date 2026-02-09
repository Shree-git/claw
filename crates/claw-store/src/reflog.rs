use claw_core::id::ObjectId;

use crate::layout::RepoLayout;
use crate::StoreError;

#[derive(Debug, Clone)]
pub struct RefLogLine {
    pub old: ObjectId,
    pub new: ObjectId,
    pub timestamp_ms: u64,
    pub author: String,
    pub message: String,
}

static ZERO_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub fn append_reflog(
    layout: &RepoLayout,
    ref_name: &str,
    old: Option<&ObjectId>,
    new: &ObjectId,
    author: &str,
    message: &str,
) -> Result<(), StoreError> {
    let path = layout.reflogs_dir().join(ref_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let old_hex = old.map_or_else(|| ZERO_HEX.to_string(), |id| id.to_hex());
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let line = format!(
        "{} {} {} {} {}\n",
        old_hex,
        new.to_hex(),
        timestamp_ms,
        author,
        message
    );
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn read_reflog(layout: &RepoLayout, ref_name: &str) -> Result<Vec<RefLogLine>, StoreError> {
    let path = layout.reflogs_dir().join(ref_name);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let mut entries = Vec::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.splitn(5, ' ').collect();
        if parts.len() < 5 {
            continue;
        }
        let old = match ObjectId::from_hex(parts[0]) {
            Ok(id) => id,
            Err(_) if parts[0] == ZERO_HEX => ObjectId::from_bytes([0; 32]),
            Err(_) => continue, // skip corrupt line
        };
        let new = match ObjectId::from_hex(parts[1]) {
            Ok(id) => id,
            Err(_) => continue, // skip corrupt line
        };
        let timestamp_ms = match parts[2].parse::<u64>() {
            Ok(t) => t,
            Err(_) => continue, // skip corrupt line
        };
        entries.push(RefLogLine {
            old,
            new,
            timestamp_ms,
            author: parts[3].to_string(),
            message: parts[4].to_string(),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::hash::content_hash;
    use claw_core::object::TypeTag;

    #[test]
    fn reflog_append_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = crate::layout::RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id1 = content_hash(TypeTag::Blob, b"a");
        let id2 = content_hash(TypeTag::Blob, b"b");

        append_reflog(&layout, "heads/main", None, &id1, "alice", "init").unwrap();
        append_reflog(&layout, "heads/main", Some(&id1), &id2, "alice", "update").unwrap();

        let entries = read_reflog(&layout, "heads/main").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].new, id1);
        assert_eq!(entries[1].old, id1);
        assert_eq!(entries[1].new, id2);
    }
}
