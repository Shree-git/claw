use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;
use crate::worktree;

#[derive(Args)]
pub struct CheckoutArgs {
    /// Branch name or ObjectId to checkout
    target: String,
    /// Force checkout even with uncommitted changes
    #[arg(long)]
    force: bool,
}

pub fn run(args: CheckoutArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    // Resolve target: try branch ref first, then object ID
    let (new_head_state, target_id) =
        if let Some(id) = store.get_ref(&format!("heads/{}", args.target))? {
            (
                HeadState::Symbolic {
                    ref_name: format!("heads/{}", args.target),
                },
                id,
            )
        } else if let Ok(id) = ObjectId::from_hex(&args.target) {
            if store.has_object(&id) {
                (HeadState::Detached { target: id }, id)
            } else {
                anyhow::bail!("object not found: {}", args.target);
            }
        } else if let Ok(id) = ObjectId::from_display(&args.target) {
            if store.has_object(&id) {
                (HeadState::Detached { target: id }, id)
            } else {
                anyhow::bail!("object not found: {}", args.target);
            }
        } else {
            anyhow::bail!("unknown branch or revision: {}", args.target);
        };

    // Load target revision
    let target_obj = store.load_object(&target_id)?;
    let target_tree = match target_obj {
        Object::Revision(ref rev) => rev
            .tree
            .ok_or_else(|| anyhow::anyhow!("revision has no tree"))?,
        _ => anyhow::bail!("target is not a revision"),
    };

    if !args.force {
        // Check for uncommitted changes: compare worktree to current HEAD's tree
        if let Some(head_id) = store.resolve_head()? {
            let head_obj = store.load_object(&head_id)?;
            if let Object::Revision(ref rev) = head_obj {
                if let Some(ref head_tree) = rev.tree {
                    let ignore = crate::ignore::IgnoreRules::load(&root);
                    let worktree_tree = worktree::scan_worktree(&store, &root, &ignore)?;
                    if worktree_tree != *head_tree {
                        let changes = claw_store::tree_diff::diff_trees(
                            &store,
                            Some(head_tree),
                            Some(&worktree_tree),
                            "",
                        )?;
                        if !changes.is_empty() {
                            anyhow::bail!(
                                "uncommitted changes ({} files). Use --force to override.",
                                changes.len()
                            );
                        }
                    }
                }
            }
        }
    }

    // Remove files tracked in current tree but not in target tree
    if let Some(head_id) = store.resolve_head()? {
        let head_obj = store.load_object(&head_id)?;
        if let Object::Revision(ref rev) = head_obj {
            if let Some(ref old_tree_id) = rev.tree {
                let old_paths = worktree::collect_tracked_paths(&store, old_tree_id, "")?;
                let new_paths = worktree::collect_tracked_paths(&store, &target_tree, "")?;
                for old_path in &old_paths {
                    if !new_paths.contains(old_path) {
                        let full = root.join(old_path);
                        let _ = std::fs::remove_file(&full);
                        // Clean empty parent dirs
                        if let Some(parent) = full.parent() {
                            let _ = remove_empty_dirs(parent, &root);
                        }
                    }
                }
            }
        }
    }

    // Materialize target tree
    worktree::materialize_tree(&store, &target_tree, &root)?;

    // Update HEAD
    store.write_head(&new_head_state)?;

    match &new_head_state {
        HeadState::Symbolic { ref_name } => {
            let branch = ref_name.strip_prefix("heads/").unwrap_or(ref_name);
            println!("Switched to branch '{}'", branch);
        }
        HeadState::Detached { target } => {
            println!("HEAD detached at {}", target);
        }
    }

    Ok(())
}

fn remove_empty_dirs(dir: &std::path::Path, stop_at: &std::path::Path) -> std::io::Result<()> {
    if dir == stop_at || !dir.starts_with(stop_at) {
        return Ok(());
    }
    if dir.is_dir() && std::fs::read_dir(dir)?.next().is_none() {
        std::fs::remove_dir(dir)?;
        if let Some(parent) = dir.parent() {
            remove_empty_dirs(parent, stop_at)?;
        }
    }
    Ok(())
}
