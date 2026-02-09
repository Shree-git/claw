use std::collections::BTreeMap;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Blob, FileMode, Patch, Tree, TreeEntry};
use claw_patch::CodecRegistry;
use claw_store::ClawStore;

use crate::MergeError;

/// Build a merged tree from base tree + merged patches.
pub fn build_merged_tree(
    store: &ClawStore,
    _registry: &CodecRegistry,
    base_tree_id: Option<&ObjectId>,
    left_tree_id: Option<&ObjectId>,
    right_tree_id: Option<&ObjectId>,
    merged_patches: &[ObjectId],
) -> Result<ObjectId, MergeError> {
    // Start from the left tree as baseline (it has left-side changes already)
    // Then apply patches that represent right-side changes
    // For simplicity and correctness, we use a different approach:
    // Start from base tree, apply all merged patches
    let mut file_map: BTreeMap<String, (Vec<u8>, FileMode)> = BTreeMap::new();

    // Flatten the base tree
    if let Some(base_id) = base_tree_id {
        flatten_tree(store, base_id, "", &mut file_map)?;
    }

    // Collect patches by target_path and apply them in order
    let mut patches_by_path: BTreeMap<String, Vec<&Patch>> = BTreeMap::new();

    let mut loaded_patches: Vec<Patch> = Vec::new();
    for patch_id in merged_patches {
        let obj = store.load_object(patch_id)?;
        if let Object::Patch(p) = obj {
            loaded_patches.push(p);
        }
    }

    for p in &loaded_patches {
        patches_by_path
            .entry(p.target_path.clone())
            .or_default()
            .push(p);
    }

    // For each path with patches, apply them
    for (path, patches) in &patches_by_path {
        let codec_id = &patches[0].codec_id;
        let codec = _registry.get(codec_id)?;

        let base_content = file_map
            .get(path)
            .map(|(data, _)| data.clone())
            .unwrap_or_default();

        let mut content = base_content;
        for patch in patches {
            content = codec.apply(&content, &patch.ops)?;
        }

        if content.is_empty() && !file_map.contains_key(path) {
            // Was a deletion
            continue;
        }

        // Determine mode: prefer what's already there, or Regular for new files
        let mode = file_map
            .get(path)
            .map(|(_, m)| *m)
            .unwrap_or(FileMode::Regular);
        file_map.insert(path.clone(), (content, mode));
    }

    // Also incorporate files from left and right trees that had no patches
    // (i.e. files that exist in one side but not in the base)
    add_unique_files(store, left_tree_id, base_tree_id, &mut file_map)?;
    add_unique_files(store, right_tree_id, base_tree_id, &mut file_map)?;

    // Build tree from flat file map
    build_tree_from_flat(store, &file_map)
}

fn add_unique_files(
    store: &ClawStore,
    tree_id: Option<&ObjectId>,
    base_tree_id: Option<&ObjectId>,
    file_map: &mut BTreeMap<String, (Vec<u8>, FileMode)>,
) -> Result<(), MergeError> {
    let Some(tid) = tree_id else {
        return Ok(());
    };
    let mut side_map: BTreeMap<String, (Vec<u8>, FileMode)> = BTreeMap::new();
    flatten_tree(store, tid, "", &mut side_map)?;

    let mut base_map: BTreeMap<String, (Vec<u8>, FileMode)> = BTreeMap::new();
    if let Some(bid) = base_tree_id {
        flatten_tree(store, bid, "", &mut base_map)?;
    }

    for (path, (data, mode)) in side_map {
        if !base_map.contains_key(&path) && !file_map.contains_key(&path) {
            file_map.insert(path, (data, mode));
        }
    }
    Ok(())
}

fn flatten_tree(
    store: &ClawStore,
    tree_id: &ObjectId,
    prefix: &str,
    out: &mut BTreeMap<String, (Vec<u8>, FileMode)>,
) -> Result<(), MergeError> {
    let obj = store.load_object(tree_id)?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => return Ok(()),
    };

    for entry in &tree.entries {
        let path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{}/{}", prefix, entry.name)
        };

        match entry.mode {
            FileMode::Directory => {
                flatten_tree(store, &entry.object_id, &path, out)?;
            }
            _ => {
                let blob_obj = store.load_object(&entry.object_id)?;
                if let Object::Blob(b) = blob_obj {
                    out.insert(path, (b.data, entry.mode));
                }
            }
        }
    }
    Ok(())
}

fn build_tree_from_flat(
    store: &ClawStore,
    file_map: &BTreeMap<String, (Vec<u8>, FileMode)>,
) -> Result<ObjectId, MergeError> {
    // Group by first path component
    let mut children: BTreeMap<String, BTreeMap<String, (Vec<u8>, FileMode)>> = BTreeMap::new();
    let mut direct_files: BTreeMap<String, (Vec<u8>, FileMode)> = BTreeMap::new();

    for (path, data) in file_map {
        if let Some((first, rest)) = path.split_once('/') {
            children
                .entry(first.to_string())
                .or_default()
                .insert(rest.to_string(), data.clone());
        } else {
            direct_files.insert(path.clone(), data.clone());
        }
    }

    let mut entries = Vec::new();

    // Create subtrees
    for (name, sub_map) in &children {
        let sub_tree_id = build_tree_from_flat(store, sub_map)?;
        entries.push(TreeEntry {
            name: name.clone(),
            mode: FileMode::Directory,
            object_id: sub_tree_id,
        });
    }

    // Create blobs
    for (name, (data, mode)) in &direct_files {
        let blob = Blob {
            data: data.clone(),
            media_type: None,
        };
        let blob_id = store.store_object(&Object::Blob(blob))?;
        entries.push(TreeEntry {
            name: name.clone(),
            mode: *mode,
            object_id: blob_id,
        });
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let tree = Tree { entries };
    let id = store.store_object(&Object::Tree(tree))?;
    Ok(id)
}
