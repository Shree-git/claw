use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Conflict, ConflictStatus, Patch, Revision};
use claw_patch::CodecRegistry;
use claw_store::ClawStore;

use crate::ancestor::find_lca;
use crate::collect::collect_patches;
use crate::group::group_patches;
use crate::rebase::commute_rebase;
use crate::MergeError;

pub struct MergeResult {
    pub revision: Revision,
    pub new_patches: Vec<ObjectId>,
    pub conflicts: Vec<Conflict>,
    pub ancestor: ObjectId,
}

/// Perform a merge of two revision heads.
pub fn merge(
    store: &ClawStore,
    registry: &CodecRegistry,
    left_head: &ObjectId,
    right_head: &ObjectId,
    author: &str,
    message: &str,
) -> Result<MergeResult, MergeError> {
    // 1. Find common ancestor
    let ancestor = find_lca(store, left_head, right_head)?
        .ok_or(MergeError::NoCommonAncestor)?;

    // 2. Collect patches from ancestor to each head
    let left_patches = collect_patches(store, &ancestor, left_head)?;
    let right_patches = collect_patches(store, &ancestor, right_head)?;

    // 3. Group by (target_path, codec_id)
    let left_groups = group_patches(store, &left_patches)?;
    let right_groups = group_patches(store, &right_patches)?;

    let mut merged_patches = Vec::new();
    let mut conflicts = Vec::new();

    // All paths from both sides
    let all_keys: std::collections::BTreeSet<_> = left_groups
        .keys()
        .chain(right_groups.keys())
        .cloned()
        .collect();

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // 4. Per-group merge
    for key in &all_keys {
        let left_ids = left_groups.get(key);
        let right_ids = right_groups.get(key);

        match (left_ids, right_ids) {
            (Some(l), None) => {
                merged_patches.extend_from_slice(l);
            }
            (None, Some(r)) => {
                merged_patches.extend_from_slice(r);
            }
            (Some(l), Some(r)) => {
                let (path, codec_id) = key;

                // Try commutation-based rebase
                match commute_rebase(store, registry, codec_id, l, r) {
                    Ok((rebased_right, _)) => {
                        // Success: left patches + rebased right patches
                        merged_patches.extend_from_slice(l);
                        for ops in rebased_right {
                            let patch = Patch {
                                target_path: path.clone(),
                                codec_id: codec_id.clone(),
                                base_object: None,
                                result_object: None,
                                ops,
                                codec_payload: None,
                            };
                            let id = store.store_object(&Object::Patch(patch))?;
                            merged_patches.push(id);
                        }
                    }
                    Err(_) => {
                        // Commute failed -- try merge3 fallback
                        match try_merge3_fallback(store, registry, &ancestor, codec_id, path, l, r) {
                            Ok(merge3_patch_id) => {
                                merged_patches.push(merge3_patch_id);
                            }
                            Err(_) => {
                                // merge3 also failed -- emit a conflict
                                let conflict = Conflict {
                                    base_revision: Some(ancestor),
                                    left_revision: *left_head,
                                    right_revision: *right_head,
                                    file_path: path.clone(),
                                    codec_id: codec_id.clone(),
                                    left_patch_ids: l.clone(),
                                    right_patch_ids: r.clone(),
                                    resolution_patch_ids: vec![],
                                    status: ConflictStatus::Open,
                                    created_at_ms: now_ms,
                                };
                                conflicts.push(conflict);
                            }
                        }
                    }
                }
            }
            (None, None) => unreachable!(),
        }
    }

    // Build merged tree from base + left + right trees using merged patches
    let ancestor_tree = {
        let obj = store.load_object(&ancestor)?;
        match obj {
            Object::Revision(rev) => rev.tree,
            _ => None,
        }
    };
    let left_tree = {
        let obj = store.load_object(left_head)?;
        match obj {
            Object::Revision(rev) => rev.tree,
            _ => None,
        }
    };
    let right_tree = {
        let obj = store.load_object(right_head)?;
        match obj {
            Object::Revision(rev) => rev.tree,
            _ => None,
        }
    };

    let tree_id = if conflicts.is_empty() {
        Some(crate::tree_build::build_merged_tree(
            store,
            registry,
            ancestor_tree.as_ref(),
            left_tree.as_ref(),
            right_tree.as_ref(),
            &merged_patches,
        )?)
    } else {
        left_tree
    };

    // Store conflict objects
    let _conflict_ids: Vec<ObjectId> = conflicts
        .iter()
        .map(|c| store.store_object(&Object::Conflict(c.clone())))
        .collect::<Result<Vec<_>, _>>()?;

    let revision = Revision {
        change_id: None,
        parents: vec![*left_head, *right_head],
        patches: merged_patches.clone(),
        snapshot_base: None,
        tree: tree_id,
        capsule_id: None,
        author: author.to_string(),
        created_at_ms: now_ms,
        summary: message.to_string(),
        policy_evidence: vec![],
    };

    Ok(MergeResult {
        revision,
        new_patches: merged_patches,
        conflicts,
        ancestor,
    })
}

/// Try merge3 fallback: reconstruct base/left/right file content, run 3-way merge.
fn try_merge3_fallback(
    store: &ClawStore,
    registry: &CodecRegistry,
    ancestor: &ObjectId,
    codec_id: &str,
    path: &str,
    left_ids: &[ObjectId],
    right_ids: &[ObjectId],
) -> Result<ObjectId, MergeError> {
    let codec = registry.get(codec_id)?;

    // Find base content from ancestor's tree
    let base_content = find_blob_content_at_path(store, ancestor, path)?
        .unwrap_or_default();

    // Apply left patches to get left content
    let mut left_content = base_content.clone();
    for id in left_ids {
        let obj = store.load_object(id)?;
        if let Object::Patch(p) = obj {
            left_content = codec.apply(&left_content, &p.ops)?;
        }
    }

    // Apply right patches to get right content
    let mut right_content = base_content.clone();
    for id in right_ids {
        let obj = store.load_object(id)?;
        if let Object::Patch(p) = obj {
            right_content = codec.apply(&right_content, &p.ops)?;
        }
    }

    // 3-way merge
    let merged_content = codec.merge3(&base_content, &left_content, &right_content)?;

    // Diff base vs merged to produce the merged patch ops
    let merged_ops = codec.diff(&base_content, &merged_content)?;

    let patch = Patch {
        target_path: path.to_string(),
        codec_id: codec_id.to_string(),
        base_object: None,
        result_object: None,
        ops: merged_ops,
        codec_payload: None,
    };
    let id = store.store_object(&Object::Patch(patch))?;
    Ok(id)
}

/// Walk the tree from a revision to find blob content at a given file path.
fn find_blob_content_at_path(
    store: &ClawStore,
    revision_id: &ObjectId,
    path: &str,
) -> Result<Option<Vec<u8>>, MergeError> {
    let obj = store.load_object(revision_id)?;
    let tree_id = match obj {
        Object::Revision(rev) => match rev.tree {
            Some(t) => t,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };

    let parts: Vec<&str> = path.split('/').collect();
    find_in_tree(store, &tree_id, &parts)
}

fn find_in_tree(
    store: &ClawStore,
    tree_id: &ObjectId,
    path_parts: &[&str],
) -> Result<Option<Vec<u8>>, MergeError> {
    if path_parts.is_empty() {
        return Ok(None);
    }

    let obj = store.load_object(tree_id)?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => return Ok(None),
    };

    let target_name = path_parts[0];
    for entry in &tree.entries {
        if entry.name == target_name {
            if path_parts.len() == 1 {
                // This is the final component - should be a blob
                if let Ok(blob_obj) = store.load_object(&entry.object_id) {
                    if let Object::Blob(b) = blob_obj {
                        return Ok(Some(b.data));
                    }
                }
                return Ok(None);
            } else {
                // Recurse into subdirectory
                return find_in_tree(store, &entry.object_id, &path_parts[1..]);
            }
        }
    }

    Ok(None)
}
