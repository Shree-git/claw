use clap::Args;

use claw_core::object::Object;
use claw_store::tree_diff::{diff_trees, ChangeKind};
use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;
use crate::ignore::IgnoreRules;
use crate::merge_state;
use crate::output;
use crate::worktree;

#[derive(Args)]
pub struct StatusArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

pub fn run(args: StatusArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    let head_state = store.read_head()?;
    let branch_name = match &head_state {
        HeadState::Symbolic { ref_name } => ref_name
            .strip_prefix("heads/")
            .unwrap_or(ref_name)
            .to_string(),
        HeadState::Detached { target } => format!("detached at {}", &target.to_hex()[..12]),
    };

    let claw_dir = store.layout().claw_dir();
    let in_merge = merge_state::exists(&claw_dir);

    // Resolve HEAD to get current tree
    let head_tree = if let Some(head_id) = store.resolve_head()? {
        let head_obj = store.load_object(&head_id)?;
        match head_obj {
            Object::Revision(ref rev) => rev.tree,
            _ => None,
        }
    } else {
        None
    };

    // Scan worktree
    let ignore = IgnoreRules::load(&root);
    let worktree_tree = worktree::scan_worktree(&store, &root, &ignore)?;

    let changes = diff_trees(&store, head_tree.as_ref(), Some(&worktree_tree), "")?;

    if args.json {
        let entries: Vec<serde_json::Value> = changes
            .iter()
            .map(|c| {
                serde_json::json!({
                    "path": c.path,
                    "status": match c.kind {
                        ChangeKind::Added => "added",
                        ChangeKind::Deleted => "deleted",
                        ChangeKind::Modified => "modified",
                        ChangeKind::TypeChanged => "type_changed",
                    },
                })
            })
            .collect();
        let output = serde_json::json!({
            "branch": branch_name,
            "in_merge": in_merge,
            "changes": entries,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("{}", output::header(&format!("On branch {}", branch_name)));

    if in_merge {
        if let Ok(ms) = merge_state::read_from(&claw_dir) {
            println!(
                "Merging: {} into {}",
                ms.merge.right_ref, ms.merge.left_ref
            );
            let unresolved: Vec<_> = ms.conflicts.iter().collect();
            if !unresolved.is_empty() {
                println!(
                    "{} unresolved conflict(s):",
                    unresolved.len()
                );
                for c in &unresolved {
                    println!("  CONFLICT: {} ({})", c.file_path, c.codec_id);
                }
            }
            println!("  (use \"claw resolve\" to manage conflicts)");
            println!("  (use \"claw snapshot\" to complete the merge)");
            println!();
        }
    }

    if changes.is_empty() {
        if head_tree.is_none() {
            println!("No commits yet.");
        } else {
            println!("Nothing to snapshot (working tree clean).");
        }
        return Ok(());
    }

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut type_changed = Vec::new();

    for c in &changes {
        match c.kind {
            ChangeKind::Added => added.push(c.path.as_str()),
            ChangeKind::Deleted => deleted.push(c.path.as_str()),
            ChangeKind::Modified => modified.push(c.path.as_str()),
            ChangeKind::TypeChanged => type_changed.push(c.path.as_str()),
        }
    }

    println!("Changes ({} file(s)):", changes.len());
    println!();

    for path in &added {
        println!("  A  {}", path);
    }
    for path in &modified {
        println!("  M  {}", path);
    }
    for path in &deleted {
        println!("  D  {}", path);
    }
    for path in &type_changed {
        println!("  T  {}", path);
    }

    println!();
    println!("  (use \"claw snapshot -m <message>\" to record)");

    Ok(())
}
