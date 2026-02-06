use claw_core::id::ObjectId;

use crate::layout::RepoLayout;
use crate::StoreError;

pub fn write_ref(layout: &RepoLayout, name: &str, target: &ObjectId) -> Result<(), StoreError> {
    let path = layout.refs_dir().join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, target.to_hex())?;
    Ok(())
}

pub fn read_ref(layout: &RepoLayout, name: &str) -> Result<Option<ObjectId>, StoreError> {
    let path = layout.refs_dir().join(name);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let id = ObjectId::from_hex(content.trim())?;
    Ok(Some(id))
}

pub fn delete_ref(layout: &RepoLayout, name: &str) -> Result<(), StoreError> {
    let path = layout.refs_dir().join(name);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn list_refs(layout: &RepoLayout, prefix: &str) -> Result<Vec<(String, ObjectId)>, StoreError> {
    let base = layout.refs_dir().join(prefix);
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    collect_refs(&base, &layout.refs_dir(), &mut results)?;
    Ok(results)
}

fn collect_refs(
    dir: &std::path::Path,
    refs_root: &std::path::Path,
    results: &mut Vec<(String, ObjectId)>,
) -> Result<(), StoreError> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_refs(&path, refs_root, results)?;
        } else if path.is_file() {
            let content = std::fs::read_to_string(&path)?;
            if let Ok(id) = ObjectId::from_hex(content.trim()) {
                let rel = path
                    .strip_prefix(refs_root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                results.push((rel, id));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::hash::content_hash;
    use claw_core::object::TypeTag;

    #[test]
    fn ref_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"test");
        write_ref(&layout, "heads/main", &id).unwrap();

        let read_back = read_ref(&layout, "heads/main").unwrap();
        assert_eq!(read_back, Some(id));
    }

    #[test]
    fn list_refs_finds_all() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id1 = content_hash(TypeTag::Blob, b"a");
        let id2 = content_hash(TypeTag::Blob, b"b");
        write_ref(&layout, "heads/main", &id1).unwrap();
        write_ref(&layout, "heads/dev", &id2).unwrap();

        let refs = list_refs(&layout, "heads").unwrap();
        assert_eq!(refs.len(), 2);
    }
}
