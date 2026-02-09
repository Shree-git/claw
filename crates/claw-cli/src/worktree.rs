use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Blob, FileMode, Tree, TreeEntry};
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
        let path = target_dir.join(&entry.name);
        match entry.mode {
            FileMode::Directory => {
                std::fs::create_dir_all(&path)?;
                materialize_tree(store, &entry.object_id, &path)?;
            }
            FileMode::Symlink => {
                let obj = store.load_object(&entry.object_id)?;
                if let Object::Blob(b) = obj {
                    let target = String::from_utf8_lossy(&b.data);
                    // Remove existing file/dir if present
                    let _ = std::fs::remove_file(&path);
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(target.as_ref(), &path)?;
                    #[cfg(not(unix))]
                    std::fs::write(&path, &b.data)?;
                }
            }
            _ => {
                let obj = store.load_object(&entry.object_id)?;
                if let Object::Blob(b) = obj {
                    // Remove dir if file should go here
                    if path.is_dir() {
                        std::fs::remove_dir_all(&path)?;
                    }
                    std::fs::write(&path, &b.data)?;
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
