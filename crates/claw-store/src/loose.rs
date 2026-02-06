use std::path::PathBuf;

use claw_core::id::ObjectId;

use crate::layout::RepoLayout;
use crate::StoreError;

pub fn loose_object_path(layout: &RepoLayout, id: &ObjectId) -> PathBuf {
    let dir = layout.objects_dir().join(id.shard_prefix());
    dir.join(id.shard_suffix())
}

pub fn write_loose_object(layout: &RepoLayout, id: &ObjectId, data: &[u8]) -> Result<(), StoreError> {
    let path = loose_object_path(layout, id);

    if path.exists() {
        return Ok(());
    }

    // Create shard directory
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Atomic write: temp file + rename
    let dir = path.parent().unwrap();
    let temp = tempfile::NamedTempFile::new_in(dir)?;
    std::fs::write(temp.path(), data)?;
    temp.persist(&path)
        .map_err(|e| StoreError::Io(e.error))?;

    Ok(())
}

pub fn read_loose_object(layout: &RepoLayout, id: &ObjectId) -> Result<Vec<u8>, StoreError> {
    let path = loose_object_path(layout, id);
    if !path.exists() {
        return Err(StoreError::ObjectNotFound(*id));
    }
    let data = std::fs::read(&path)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::hash::content_hash;
    use claw_core::object::TypeTag;

    #[test]
    fn write_and_read_loose_object() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let data = b"test object data";
        let id = content_hash(TypeTag::Blob, data);
        write_loose_object(&layout, &id, data).unwrap();

        let read_back = read_loose_object(&layout, &id).unwrap();
        assert_eq!(read_back, data);
    }

    #[test]
    fn missing_object_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"nonexistent");
        assert!(read_loose_object(&layout, &id).is_err());
    }
}
