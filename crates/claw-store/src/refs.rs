use claw_core::id::ObjectId;

use crate::fs_util;
use crate::layout::RepoLayout;
use crate::StoreError;

fn validate_ref_path(name: &str, allow_empty: bool) -> Result<(), StoreError> {
    use std::path::{Component, Path};

    if name.is_empty() {
        if allow_empty {
            return Ok(());
        }
        return Err(StoreError::InvalidRefName(name.to_string()));
    }

    if name.contains('\0') || name.contains('\\') {
        return Err(StoreError::InvalidRefName(name.to_string()));
    }

    let path = Path::new(name);
    if path.is_absolute() {
        return Err(StoreError::InvalidRefName(name.to_string()));
    }

    let mut saw_component = false;
    for component in path.components() {
        match component {
            Component::Normal(seg) => {
                saw_component = true;
                let seg_str = seg
                    .to_str()
                    .ok_or_else(|| StoreError::InvalidRefName(name.to_string()))?;
                if seg_str.is_empty() || seg_str == "." || seg_str == ".." {
                    return Err(StoreError::InvalidRefName(name.to_string()));
                }
                if seg_str.len() > 255
                    || seg_str.ends_with([' ', '.'])
                    || has_windows_reserved_char(seg_str)
                    || is_windows_reserved_device_name(seg_str)
                {
                    return Err(StoreError::InvalidRefName(name.to_string()));
                }
                if seg_str
                    .chars()
                    .any(|c| c.is_control() || c == '/' || c == '\\')
                {
                    return Err(StoreError::InvalidRefName(name.to_string()));
                }
            }
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(StoreError::InvalidRefName(name.to_string()));
            }
        }
    }

    if !saw_component && !allow_empty {
        return Err(StoreError::InvalidRefName(name.to_string()));
    }

    if name.contains("//") {
        return Err(StoreError::InvalidRefName(name.to_string()));
    }

    Ok(())
}

fn has_windows_reserved_char(name: &str) -> bool {
    name.chars()
        .any(|ch| matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || is_numbered_windows_device(&upper, "COM")
        || is_numbered_windows_device(&upper, "LPT")
}

fn is_numbered_windows_device(name: &str, prefix: &str) -> bool {
    let Some(suffix) = name.strip_prefix(prefix) else {
        return false;
    };
    matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
}

pub fn validate_ref_name(name: &str) -> Result<(), StoreError> {
    validate_ref_path(name, false)
}

pub(crate) fn validate_existing_ref_path_portable(
    layout: &RepoLayout,
    name: &str,
) -> Result<(), StoreError> {
    validate_ref_name(name)?;
    ensure_ref_path_has_no_case_collision(layout, name)
}

pub fn write_ref(layout: &RepoLayout, name: &str, target: &ObjectId) -> Result<(), StoreError> {
    validate_existing_ref_path_portable(layout, name)?;
    let path = layout.refs_dir().join(name);

    let content = format!("{}\n", target.to_hex());
    fs_util::write_atomic(&path, content.as_bytes())
}

pub fn read_ref(layout: &RepoLayout, name: &str) -> Result<Option<ObjectId>, StoreError> {
    validate_existing_ref_path_portable(layout, name)?;
    let path = layout.refs_dir().join(name);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let id = ObjectId::from_hex(content.trim())?;
    Ok(Some(id))
}

pub fn delete_ref(layout: &RepoLayout, name: &str) -> Result<(), StoreError> {
    validate_existing_ref_path_portable(layout, name)?;
    let path = layout.refs_dir().join(name);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn update_ref_cas(
    layout: &RepoLayout,
    name: &str,
    expected_old: Option<&ObjectId>,
    new_target: &ObjectId,
    author: &str,
    message: &str,
) -> Result<(), StoreError> {
    use crate::lockfile::LockFile;
    use crate::reflog;

    validate_existing_ref_path_portable(layout, name)?;
    let ref_path = layout.refs_dir().join(name);
    let _lock = LockFile::acquire(&ref_path)?;

    let current = read_ref(layout, name)?;

    match (expected_old, &current) {
        (None, None) => {}
        (Some(expected), Some(actual)) if expected == actual => {}
        (expected, actual) => {
            return Err(StoreError::RefCasConflict {
                expected: expected
                    .map(|id| id.to_hex())
                    .unwrap_or_else(|| "none".to_string()),
                actual: actual
                    .as_ref()
                    .map(|id| id.to_hex())
                    .unwrap_or_else(|| "none".to_string()),
            });
        }
    }

    write_ref(layout, name, new_target)?;
    reflog::append_reflog(layout, name, current.as_ref(), new_target, author, message)?;
    Ok(())
}

fn ensure_ref_path_has_no_case_collision(
    layout: &RepoLayout,
    name: &str,
) -> Result<(), StoreError> {
    let mut dir = layout.refs_dir().to_path_buf();
    for segment in name.split('/') {
        if !dir.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(existing) = file_name.to_str() else {
                continue;
            };
            if existing != segment && existing.eq_ignore_ascii_case(segment) {
                let requested_path = dir.join(segment);
                let requested = requested_path
                    .strip_prefix(layout.refs_dir())
                    .unwrap_or(requested_path.as_path())
                    .to_string_lossy()
                    .to_string();
                let existing_path = entry.path();
                let existing = existing_path
                    .strip_prefix(layout.refs_dir())
                    .unwrap_or(existing_path.as_path())
                    .to_string_lossy()
                    .to_string();
                return Err(StoreError::RefNameCollision {
                    requested,
                    existing,
                });
            }
        }
        dir.push(segment);
    }
    Ok(())
}

pub fn list_refs(layout: &RepoLayout, prefix: &str) -> Result<Vec<(String, ObjectId)>, StoreError> {
    validate_ref_path(prefix, true)?;
    if !prefix.is_empty() {
        ensure_ref_path_has_no_case_collision(layout, prefix)?;
    }
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
                    .map_err(|_| StoreError::RefPathEscapesRoot(path.clone()))?
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

    #[test]
    fn rejects_traversal_ref_names() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"x");
        let err = write_ref(&layout, "../outside", &id).unwrap_err();
        assert!(matches!(err, StoreError::InvalidRefName(_)));

        let err = write_ref(&layout, "heads/../main", &id).unwrap_err();
        assert!(matches!(err, StoreError::InvalidRefName(_)));

        let err = list_refs(&layout, "../../").unwrap_err();
        assert!(matches!(err, StoreError::InvalidRefName(_)));
    }

    #[test]
    fn allows_common_ref_shapes() {
        validate_ref_name("heads/main").unwrap();
        validate_ref_name("changes/01J00000000000000000000000").unwrap();
        validate_ref_name("capsules/by-revision/abcdef0123456789").unwrap();
        validate_ref_path("", true).unwrap();
    }

    #[test]
    fn rejects_non_portable_ref_components() {
        for name in [
            "heads/name:stream",
            "heads/name*glob",
            "heads/name?query",
            "heads/name<in",
            "heads/name>out",
            "heads/pipe|name",
            "heads/quoted\"name",
            "heads/trailing-dot.",
            "heads/trailing-space ",
            "heads/CON",
            "heads/con.txt",
            "heads/PRN",
            "heads/AUX",
            "heads/NUL",
            "heads/COM1",
            "heads/com9.log",
            "heads/LPT1",
            "heads/lpt9.txt",
        ] {
            let err = validate_ref_name(name).expect_err("ref name should be rejected");
            assert!(matches!(err, StoreError::InvalidRefName(_)), "{name}");
        }

        let overlong_component = format!("heads/{}", "a".repeat(256));
        let err = validate_ref_name(&overlong_component)
            .expect_err("overlong ref component should be rejected");
        assert!(matches!(err, StoreError::InvalidRefName(_)));
    }

    #[test]
    fn write_ref_rejects_case_insensitive_sibling_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let main = content_hash(TypeTag::Blob, b"main");
        let other = content_hash(TypeTag::Blob, b"other");
        write_ref(&layout, "heads/main", &main).unwrap();

        let err = write_ref(&layout, "heads/MAIN", &other)
            .expect_err("case-insensitive sibling ref should be rejected");

        assert!(matches!(err, StoreError::RefNameCollision { .. }));
        assert_eq!(read_ref(&layout, "heads/main").unwrap(), Some(main));
        assert!(!std::fs::read_dir(layout.refs_dir().join("heads"))
            .unwrap()
            .any(|entry| entry.unwrap().file_name() == "MAIN"));
    }

    #[test]
    fn update_ref_cas_rejects_case_insensitive_parent_collisions_before_locking() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let main = content_hash(TypeTag::Blob, b"main");
        let other = content_hash(TypeTag::Blob, b"other");
        write_ref(&layout, "heads/main", &main).unwrap();

        let err = update_ref_cas(&layout, "HEADS/dev", None, &other, "test", "case collision")
            .expect_err("case-insensitive parent ref should be rejected");

        assert!(matches!(err, StoreError::RefNameCollision { .. }));
        assert!(!layout.refs_dir().join("HEADS/dev.lock").exists());
    }

    #[test]
    fn update_ref_cas_creates_missing_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"policy");
        update_ref_cas(
            &layout,
            "policies/release-gate",
            None,
            &id,
            "policy",
            "policy create/update",
        )
        .unwrap();

        let read_back = read_ref(&layout, "policies/release-gate").unwrap();
        assert_eq!(read_back, Some(id));
    }

    #[test]
    fn stale_ref_lock_blocks_cas_without_changing_existing_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let old = content_hash(TypeTag::Blob, b"old");
        let new = content_hash(TypeTag::Blob, b"new");
        write_ref(&layout, "heads/main", &old).unwrap();
        let lock_path = layout.refs_dir().join("heads/main.lock");
        std::fs::write(&lock_path, b"interrupted update").unwrap();

        let err = update_ref_cas(
            &layout,
            "heads/main",
            Some(&old),
            &new,
            "test",
            "simulate stale lock",
        )
        .expect_err("stale lock should block CAS");

        assert!(matches!(err, StoreError::LockContention(_)));
        assert_eq!(read_ref(&layout, "heads/main").unwrap(), Some(old));
    }
}
