use std::collections::BTreeMap;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::FileMode;

use crate::ClawStore;
use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    TypeChanged,
}

#[derive(Debug, Clone)]
pub struct TreeChange {
    pub path: String,
    pub kind: ChangeKind,
    pub old_id: Option<ObjectId>,
    pub new_id: Option<ObjectId>,
    pub old_mode: Option<FileMode>,
    pub new_mode: Option<FileMode>,
}

pub fn diff_trees(
    store: &ClawStore,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    prefix: &str,
) -> Result<Vec<TreeChange>, StoreError> {
    let old_entries = match old_tree {
        Some(id) => flatten_tree_entries(store, id)?,
        None => BTreeMap::new(),
    };
    let new_entries = match new_tree {
        Some(id) => flatten_tree_entries(store, id)?,
        None => BTreeMap::new(),
    };

    let mut changes = Vec::new();

    let mut all_names: Vec<&String> = old_entries.keys().chain(new_entries.keys()).collect();
    all_names.sort();
    all_names.dedup();

    for name in all_names {
        let full_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };

        match (old_entries.get(name), new_entries.get(name)) {
            (None, Some((new_id, new_mode))) => {
                if *new_mode == FileMode::Directory {
                    let sub_changes = diff_trees(store, None, Some(new_id), &full_path)?;
                    changes.extend(sub_changes);
                } else {
                    changes.push(TreeChange {
                        path: full_path,
                        kind: ChangeKind::Added,
                        old_id: None,
                        new_id: Some(*new_id),
                        old_mode: None,
                        new_mode: Some(*new_mode),
                    });
                }
            }
            (Some((old_id, old_mode)), None) => {
                if *old_mode == FileMode::Directory {
                    let sub_changes = diff_trees(store, Some(old_id), None, &full_path)?;
                    changes.extend(sub_changes);
                } else {
                    changes.push(TreeChange {
                        path: full_path,
                        kind: ChangeKind::Deleted,
                        old_id: Some(*old_id),
                        new_id: None,
                        old_mode: Some(*old_mode),
                        new_mode: None,
                    });
                }
            }
            (Some((old_id, old_mode)), Some((new_id, new_mode))) => {
                if old_mode != new_mode && (*old_mode == FileMode::Directory || *new_mode == FileMode::Directory) {
                    // Type changed between file and directory
                    if *old_mode == FileMode::Directory {
                        let sub_changes = diff_trees(store, Some(old_id), None, &full_path)?;
                        changes.extend(sub_changes);
                    } else {
                        changes.push(TreeChange {
                            path: full_path.clone(),
                            kind: ChangeKind::Deleted,
                            old_id: Some(*old_id),
                            new_id: None,
                            old_mode: Some(*old_mode),
                            new_mode: None,
                        });
                    }
                    if *new_mode == FileMode::Directory {
                        let sub_changes = diff_trees(store, None, Some(new_id), &full_path)?;
                        changes.extend(sub_changes);
                    } else {
                        changes.push(TreeChange {
                            path: full_path,
                            kind: ChangeKind::Added,
                            old_id: None,
                            new_id: Some(*new_id),
                            old_mode: None,
                            new_mode: Some(*new_mode),
                        });
                    }
                } else if *old_mode == FileMode::Directory && *new_mode == FileMode::Directory {
                    if old_id != new_id {
                        let sub_changes = diff_trees(store, Some(old_id), Some(new_id), &full_path)?;
                        changes.extend(sub_changes);
                    }
                } else if old_id != new_id {
                    changes.push(TreeChange {
                        path: full_path,
                        kind: if old_mode != new_mode {
                            ChangeKind::TypeChanged
                        } else {
                            ChangeKind::Modified
                        },
                        old_id: Some(*old_id),
                        new_id: Some(*new_id),
                        old_mode: Some(*old_mode),
                        new_mode: Some(*new_mode),
                    });
                } else if old_mode != new_mode {
                    changes.push(TreeChange {
                        path: full_path,
                        kind: ChangeKind::TypeChanged,
                        old_id: Some(*old_id),
                        new_id: Some(*new_id),
                        old_mode: Some(*old_mode),
                        new_mode: Some(*new_mode),
                    });
                }
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(changes)
}

fn flatten_tree_entries(
    store: &ClawStore,
    tree_id: &ObjectId,
) -> Result<BTreeMap<String, (ObjectId, FileMode)>, StoreError> {
    let obj = store.load_object(tree_id)?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => return Ok(BTreeMap::new()),
    };
    let mut map = BTreeMap::new();
    for entry in tree.entries {
        map.insert(entry.name, (entry.object_id, entry.mode));
    }
    Ok(map)
}
