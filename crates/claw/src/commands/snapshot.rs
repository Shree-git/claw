use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Patch, Revision};
use claw_patch::CodecRegistry;
use claw_store::tree_diff::{diff_trees, ChangeKind};
use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;
use crate::conflict_writer;
use crate::ignore::IgnoreRules;
use crate::merge_state;
use crate::worktree;

#[derive(Args)]
pub struct SnapshotArgs {
    /// Snapshot message
    #[arg(short, long)]
    message: String,
    /// Author name
    #[arg(short, long, default_value = "claw")]
    author: String,
    /// Optional change ID to associate
    #[arg(short, long)]
    change: Option<String>,
    /// Output result as JSON
    #[arg(long)]
    json: bool,
}

pub fn run(args: SnapshotArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let registry = CodecRegistry::default();
    let ignore = IgnoreRules::load(&root);

    let claw_dir = store.layout().claw_dir();
    let is_merge_completion = merge_state::exists(&claw_dir);

    // Resolve HEAD to get branch ref name
    let head_state = store.read_head()?;
    let branch_ref = match &head_state {
        HeadState::Symbolic { ref_name } => ref_name.clone(),
        HeadState::Detached { .. } => anyhow::bail!(
            "cannot snapshot in detached HEAD state. Run `claw checkout <branch>` before snapshotting."
        ),
    };

    let old_tip = store.get_ref(&branch_ref)?;

    // Scan worktree
    let new_tree = worktree::scan_worktree(&store, &root, &ignore)?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    let change_id = args
        .change
        .as_ref()
        .and_then(|s| claw_core::id::ChangeId::from_string(s).ok());

    if is_merge_completion {
        // Merge completion: create two-parent revision
        let ms = merge_state::read_from(&claw_dir)?;
        let left_rev = ObjectId::from_hex(&ms.merge.left_revision)?;
        let right_rev = ObjectId::from_hex(&ms.merge.right_revision)?;

        let revision = Revision {
            change_id,
            parents: vec![left_rev, right_rev],
            patches: vec![],
            snapshot_base: None,
            tree: Some(new_tree),
            capsule_id: None,
            author: args.author.clone(),
            created_at_ms: now_ms,
            summary: args.message.clone(),
            policy_evidence: vec![],
        };
        let rev_id = store.store_object(&Object::Revision(revision))?;
        store.update_ref_cas(
            &branch_ref,
            old_tip.as_ref(),
            &rev_id,
            &args.author,
            &args.message,
        )?;

        // Clean up merge state and sidecars
        merge_state::remove(&claw_dir)?;
        // Remove conflict sidecars
        for conflict in &ms.conflicts {
            let conflicted_path = root.join(&conflict.file_path);
            let base_path = conflict_writer::sidecar_path(&conflicted_path, "BASE");
            let right_path = conflict_writer::sidecar_path(&conflicted_path, "RIGHT");
            let _ = std::fs::remove_file(base_path);
            let _ = std::fs::remove_file(right_path);
        }

        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "action": "snapshot",
                    "snapshot_created": true,
                    "merge_resolved": true,
                    "revision_id": rev_id.to_string(),
                    "branch": branch_ref,
                    "parents": [left_rev.to_string(), right_rev.to_string()],
                }))?
            );
        } else {
            println!("Merge resolved: {rev_id}");
        }
        return Ok(());
    }

    // Normal snapshot
    let mut patches = Vec::new();
    let mut changed_files: Option<usize> = None;

    if let Some(ref tip_id) = old_tip {
        // Get old tree from tip revision
        let tip_obj = store.load_object(tip_id)?;
        let old_tree_id = match tip_obj {
            Object::Revision(ref rev) => rev.tree,
            _ => None,
        };

        let changes = diff_trees(&store, old_tree_id.as_ref(), Some(&new_tree), "")?;
        changed_files = Some(changes.len());

        if changes.is_empty() {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "snapshot",
                        "snapshot_created": false,
                        "reason": "clean",
                        "branch": branch_ref,
                        "revision_id": serde_json::Value::Null,
                        "merge_resolved": false,
                        "patches": 0,
                        "changed_files": 0,
                    }))?
                );
            } else {
                println!("No changes to snapshot.");
            }
            return Ok(());
        }

        for change in &changes {
            let ext = change.path.rsplit('.').next().unwrap_or("");
            let codec = registry.get_by_extension(ext);

            if let Some(codec) = codec {
                let old_content = match (change.kind.clone(), change.old_id) {
                    (ChangeKind::Modified, Some(id))
                    | (ChangeKind::TypeChanged, Some(id))
                    | (ChangeKind::Deleted, Some(id)) => {
                        let obj = store.load_object(&id)?;
                        match obj {
                            Object::Blob(b) => b.data,
                            _ => vec![],
                        }
                    }
                    _ => vec![],
                };
                let new_content = match (change.kind.clone(), change.new_id) {
                    (ChangeKind::Added, Some(id))
                    | (ChangeKind::Modified, Some(id))
                    | (ChangeKind::TypeChanged, Some(id)) => {
                        let obj = store.load_object(&id)?;
                        match obj {
                            Object::Blob(b) => b.data,
                            _ => vec![],
                        }
                    }
                    _ => vec![],
                };

                if let Ok(ops) = codec.diff(&old_content, &new_content) {
                    if !ops.is_empty() {
                        let patch = Patch {
                            target_path: change.path.clone(),
                            codec_id: codec.id().to_string(),
                            base_object: change.old_id,
                            result_object: change.new_id,
                            ops,
                            codec_payload: None,
                        };
                        let patch_id = store.store_object(&Object::Patch(patch))?;
                        patches.push(patch_id);
                    }
                }
            }
            // No codec for this extension - still track it via the tree change
        }
    }

    let patch_count = patches.len();
    let revision = Revision {
        change_id,
        parents: old_tip.into_iter().collect(),
        patches,
        snapshot_base: None,
        tree: Some(new_tree),
        capsule_id: None,
        author: args.author.clone(),
        created_at_ms: now_ms,
        summary: args.message.clone(),
        policy_evidence: vec![],
    };

    let rev_id = store.store_object(&Object::Revision(revision))?;
    store.update_ref_cas(
        &branch_ref,
        old_tip.as_ref(),
        &rev_id,
        &args.author,
        &args.message,
    )?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "snapshot",
                "snapshot_created": true,
                "merge_resolved": false,
                "revision_id": rev_id.to_string(),
                "branch": branch_ref,
                "patches": patch_count,
                "changed_files": changed_files,
            }))?
        );
    } else {
        println!("Snapshot: {rev_id}");
    }
    Ok(())
}
