use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{validate_tree_entry_name, Blob, FileMode, Tree, TreeEntry};
use claw_store::ClawStore;

use crate::ignore::IgnoreRules;

/// Scan working directory, store blobs/trees, return root tree ObjectId.
pub fn scan_worktree(
    store: &ClawStore,
    root: &Path,
    ignore: &IgnoreRules,
) -> anyhow::Result<ObjectId> {
    scan_dir(store, root, root, ignore)
}

fn scan_dir(
    store: &ClawStore,
    dir: &Path,
    repo_root: &Path,
    ignore: &IgnoreRules,
) -> anyhow::Result<ObjectId> {
    let mut entries_map: BTreeMap<String, TreeEntry> = BTreeMap::new();

    let mut dir_entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        validate_tree_entry_name(&file_name)?;
        let path = entry.path();
        let rel_path = path
            .strip_prefix(repo_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let ft = entry.file_type()?;
        let is_dir = ft.is_dir();

        if ignore.is_ignored(&rel_path, is_dir) {
            continue;
        }

        if ft.is_symlink() {
            let target = std::fs::read_link(&path)?;
            let target_str = target.to_string_lossy().to_string();
            let blob = Blob {
                data: target_str.into_bytes(),
                media_type: None,
            };
            let id = store.store_object(&Object::Blob(blob))?;
            entries_map.insert(
                file_name.clone(),
                TreeEntry {
                    name: file_name,
                    mode: FileMode::Symlink,
                    object_id: id,
                },
            );
        } else if is_dir {
            let sub_tree_id = scan_dir(store, &path, repo_root, ignore)?;
            entries_map.insert(
                file_name.clone(),
                TreeEntry {
                    name: file_name,
                    mode: FileMode::Directory,
                    object_id: sub_tree_id,
                },
            );
        } else if ft.is_file() {
            let data = std::fs::read(&path)?;
            let mode = detect_file_mode(&path);
            let blob = Blob {
                data,
                media_type: None,
            };
            let id = store.store_object(&Object::Blob(blob))?;
            entries_map.insert(
                file_name.clone(),
                TreeEntry {
                    name: file_name,
                    mode,
                    object_id: id,
                },
            );
        }
    }

    let tree = Tree {
        entries: entries_map.into_values().collect(),
    };
    let id = store.store_object(&Object::Tree(tree))?;
    Ok(id)
}

fn detect_file_mode(path: &Path) -> FileMode {
    #[cfg(not(unix))]
    let _ = path;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.permissions().mode() & 0o111 != 0 {
                return FileMode::Executable;
            }
        }
    }
    FileMode::Regular
}

/// Materialize a stored tree to the filesystem.
pub fn materialize_tree(
    store: &ClawStore,
    tree_id: &ObjectId,
    target_dir: &Path,
) -> anyhow::Result<()> {
    let obj = store.load_object(tree_id)?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => anyhow::bail!("expected tree object"),
    };

    for entry in &tree.entries {
        validate_tree_entry_name(&entry.name)?;
        let path = target_dir.join(&entry.name);
        match entry.mode {
            FileMode::Directory => {
                remove_non_directory(&path)?;
                std::fs::create_dir_all(&path)?;
                materialize_tree(store, &entry.object_id, &path)?;
            }
            FileMode::Symlink => {
                let obj = store.load_object(&entry.object_id)?;
                if let Object::Blob(b) = obj {
                    remove_path_if_exists(&path)?;
                    #[cfg(unix)]
                    {
                        let target = String::from_utf8_lossy(&b.data);
                        std::os::unix::fs::symlink(target.as_ref(), &path)?;
                    }
                    #[cfg(not(unix))]
                    write_file_atomic(&path, &b.data)?;
                }
            }
            _ => {
                let obj = store.load_object(&entry.object_id)?;
                if let Object::Blob(b) = obj {
                    remove_path_if_exists(&path)?;
                    write_file_atomic(&path, &b.data)?;
                    #[cfg(unix)]
                    if entry.mode == FileMode::Executable {
                        use std::os::unix::fs::PermissionsExt;
                        let mut perms = std::fs::metadata(&path)?.permissions();
                        perms.set_mode(perms.mode() | 0o111);
                        std::fs::set_permissions(&path, perms)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
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

fn remove_non_directory(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if metadata.is_dir() {
        return Ok(());
    }

    std::fs::remove_file(path)
}

fn remove_path_if_exists(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Collect all tracked file paths from a tree.
pub fn collect_tracked_paths(
    store: &ClawStore,
    tree_id: &ObjectId,
    prefix: &str,
) -> anyhow::Result<HashSet<String>> {
    let mut paths = HashSet::new();
    let obj = store.load_object(tree_id)?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => return Ok(paths),
    };

    for entry in &tree.entries {
        let full_path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", prefix, entry.name)
        };

        match entry.mode {
            FileMode::Directory => {
                let sub = collect_tracked_paths(store, &entry.object_id, &full_path)?;
                paths.extend(sub);
            }
            _ => {
                paths.insert(full_path);
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::object::Object;
    use claw_core::types::{Blob, FileMode, Tree, TreeEntry};
    use claw_store::ClawStore;

    #[test]
    fn materialize_tree_writes_nested_files_atomically() {
        let repo = tempfile::tempdir().unwrap();
        let store = ClawStore::init(repo.path()).unwrap();

        let blob_id = store
            .store_object(&Object::Blob(Blob {
                data: b"hello\n".to_vec(),
                media_type: None,
            }))
            .unwrap();
        let child_tree_id = store
            .store_object(&Object::Tree(Tree {
                entries: vec![TreeEntry {
                    name: "file.txt".to_string(),
                    mode: FileMode::Regular,
                    object_id: blob_id,
                }],
            }))
            .unwrap();
        let root_tree_id = store
            .store_object(&Object::Tree(Tree {
                entries: vec![TreeEntry {
                    name: "nested".to_string(),
                    mode: FileMode::Directory,
                    object_id: child_tree_id,
                }],
            }))
            .unwrap();

        let out = repo.path().join("checkout");
        materialize_tree(&store, &root_tree_id, &out).unwrap();

        assert_eq!(
            std::fs::read_to_string(out.join("nested").join("file.txt")).unwrap(),
            "hello\n"
        );
    }
}
