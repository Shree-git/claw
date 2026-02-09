use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_merge::emit::merge;
use claw_patch::CodecRegistry;
use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;
use crate::conflict_writer;
use crate::merge_state::{self, ConflictEntry, MergeInfo, MergeState};
use crate::worktree;

#[derive(Args)]
pub struct IntegrateArgs {
    /// Left ref (default: HEAD's branch)
    #[arg(long)]
    left: Option<String>,
    /// Right ref to integrate
    #[arg(long)]
    right: String,
    /// Author name
    #[arg(short, long, default_value = "anonymous")]
    author: String,
    /// Merge message
    #[arg(short, long, default_value = "Integrate changes")]
    message: String,
}

pub fn run(args: IntegrateArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let registry = CodecRegistry::default();

    // Resolve left ref: default to HEAD's branch
    let left_ref = match args.left {
        Some(r) => r,
        None => {
            let head = store.read_head()?;
            match head {
                HeadState::Symbolic { ref_name } => ref_name,
                HeadState::Detached { .. } => {
                    anyhow::bail!("cannot integrate in detached HEAD state; use --left to specify")
                }
            }
        }
    };

    let left_id = store
        .get_ref(&left_ref)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", left_ref))?;
    let right_id = store
        .get_ref(&args.right)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.right))?;

    let result = merge(&store, &registry, &left_id, &right_id, &args.author, &args.message)?;

    if result.conflicts.is_empty() {
        // Clean merge: store revision, materialize tree, advance ref
        let rev_id = store.store_object(&Object::Revision(result.revision))?;
        store.update_ref_cas(&left_ref, Some(&left_id), &rev_id, &args.author, &args.message)?;

        // Materialize merged tree
        if let Some(tree_id) = store.load_object(&rev_id)?.as_revision_tree() {
            worktree::materialize_tree(&store, &tree_id, &root)?;
        }

        println!("Integrated successfully: {rev_id}");
    } else {
        // Conflicted merge: write conflict artifacts, MERGE_STATE.toml, do NOT advance ref
        let mut conflict_entries = Vec::new();

        for conflict in &result.conflicts {
            let base_content = load_file_from_revision(&store, &result.ancestor, &conflict.file_path);
            let left_content = load_file_from_revision(&store, &left_id, &conflict.file_path);
            let right_content = load_file_from_revision(&store, &right_id, &conflict.file_path);

            let conflict_id = claw_core::id::ConflictId::new().to_string();

            match conflict.codec_id.as_str() {
                "json/tree" => {
                    conflict_writer::write_json_conflict(
                        &root,
                        &conflict.file_path,
                        &base_content,
                        &left_content,
                        &right_content,
                    )?;
                }
                "binary" => {
                    conflict_writer::write_binary_conflict(
                        &root,
                        &conflict.file_path,
                        &base_content,
                        &left_content,
                        &right_content,
                    )?;
                }
                _ => {
                    conflict_writer::write_text_conflict(
                        &root,
                        &conflict.file_path,
                        &base_content,
                        &left_content,
                        &right_content,
                        &left_ref,
                        &args.right,
                    )?;
                }
            }

            conflict_entries.push(ConflictEntry {
                file_path: conflict.file_path.clone(),
                conflict_id,
                codec_id: conflict.codec_id.clone(),
            });
        }

        // Write MERGE_STATE.toml
        let merge_state = MergeState {
            merge: MergeInfo {
                left_ref: left_ref.clone(),
                right_ref: args.right.clone(),
                left_revision: left_id.to_hex(),
                right_revision: right_id.to_hex(),
                base_revision: result.ancestor.to_hex(),
            },
            conflicts: conflict_entries,
        };
        merge_state::write_to(&store.layout().claw_dir(), &merge_state)?;

        // Also materialize non-conflicting changes from the merged tree
        // Use the left tree as the base for the working copy
        let left_obj = store.load_object(&left_id)?;
        if let Object::Revision(ref rev) = left_obj {
            if let Some(ref tree_id) = rev.tree {
                worktree::materialize_tree(&store, tree_id, &root)?;
            }
        }

        println!(
            "Merge has {} conflict(s). Resolve them and run `claw snapshot` to complete.",
            result.conflicts.len()
        );
        for c in &result.conflicts {
            println!("  CONFLICT: {} ({})", c.file_path, c.codec_id);
        }
    }

    Ok(())
}

fn load_file_from_revision(store: &ClawStore, rev_id: &ObjectId, path: &str) -> Vec<u8> {
    let obj = match store.load_object(rev_id) {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let tree_id = match obj {
        Object::Revision(ref rev) => match rev.tree {
            Some(t) => t,
            None => return vec![],
        },
        _ => return vec![],
    };
    find_blob_in_tree(store, &tree_id, path).unwrap_or_default()
}

fn find_blob_in_tree(store: &ClawStore, tree_id: &ObjectId, path: &str) -> Option<Vec<u8>> {
    let parts: Vec<&str> = path.split('/').collect();
    find_blob_recursive(store, tree_id, &parts)
}

fn find_blob_recursive(store: &ClawStore, tree_id: &ObjectId, parts: &[&str]) -> Option<Vec<u8>> {
    if parts.is_empty() {
        return None;
    }
    let obj = store.load_object(tree_id).ok()?;
    let tree = match obj {
        Object::Tree(t) => t,
        _ => return None,
    };
    for entry in &tree.entries {
        if entry.name == parts[0] {
            if parts.len() == 1 {
                let blob_obj = store.load_object(&entry.object_id).ok()?;
                if let Object::Blob(b) = blob_obj {
                    return Some(b.data);
                }
                return None;
            } else {
                return find_blob_recursive(store, &entry.object_id, &parts[1..]);
            }
        }
    }
    None
}

// Helper trait for Object
trait ObjectExt {
    fn as_revision_tree(&self) -> Option<ObjectId>;
}

impl ObjectExt for Object {
    fn as_revision_tree(&self) -> Option<ObjectId> {
        match self {
            Object::Revision(rev) => rev.tree,
            _ => None,
        }
    }
}
