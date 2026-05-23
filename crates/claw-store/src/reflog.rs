use claw_core::id::ObjectId;

use crate::layout::RepoLayout;
use crate::refs;
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
    refs::validate_existing_ref_path_portable(layout, ref_name)?;
    let path = layout.reflogs_dir().join(ref_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let old_hex = old.map_or_else(|| ZERO_HEX.to_string(), |id| id.to_hex());
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let author = sanitize_reflog_author(author);
    let message = sanitize_reflog_message(message);
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
    file.sync_all()?;
    if let Some(parent) = path.parent() {
        if let Ok(parent_dir) = std::fs::File::open(parent) {
            parent_dir.sync_all()?;
        }
    }
    Ok(())
}

fn sanitize_reflog_author(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\0' || ch.is_whitespace() {
                '_'
            } else {
                ch
            }
        })
        .collect()
}

fn sanitize_reflog_message(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\0' | '\r' | '\n' => ' ',
            _ => ch,
        })
        .collect()
}

pub fn read_reflog(layout: &RepoLayout, ref_name: &str) -> Result<Vec<RefLogLine>, StoreError> {
    refs::validate_existing_ref_path_portable(layout, ref_name)?;
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

    #[test]
    fn reflog_rejects_invalid_ref_names_without_touching_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = crate::layout::RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();
        let id = content_hash(TypeTag::Blob, b"a");

        for name in ["../outside", "heads/../main", r"heads\main", "heads/CON"] {
            let err = append_reflog(&layout, name, None, &id, "alice", "bad ref")
                .expect_err("invalid reflog ref name should be rejected");
            assert!(matches!(err, StoreError::InvalidRefName(_)));
        }

        assert!(!layout.root().join("outside").exists());
        assert!(!layout.reflogs_dir().join(r"heads\main").exists());
        assert!(!layout.reflogs_dir().join("heads/CON").exists());
    }

    #[test]
    fn reflog_rejects_case_insensitive_ref_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = crate::layout::RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"main");
        crate::refs::write_ref(&layout, "heads/main", &id).unwrap();

        let err = read_reflog(&layout, "heads/MAIN")
            .expect_err("wrong-case reflog lookup should be rejected");
        assert!(matches!(err, StoreError::RefNameCollision { .. }));

        let err = append_reflog(&layout, "heads/MAIN", None, &id, "alice", "case collision")
            .expect_err("wrong-case reflog append should be rejected");
        assert!(matches!(err, StoreError::RefNameCollision { .. }));
    }

    #[test]
    fn reflog_normalizes_line_delimited_text_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = crate::layout::RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();
        let id = content_hash(TypeTag::Blob, b"a");

        append_reflog(
            &layout,
            "heads/main",
            None,
            &id,
            "alice\nmallory",
            "first line\nforged line",
        )
        .unwrap();

        let raw = std::fs::read_to_string(layout.reflogs_dir().join("heads/main")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        let entries = read_reflog(&layout, "heads/main").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].author, "alice_mallory");
        assert_eq!(entries[0].message, "first line forged line");
    }
}
