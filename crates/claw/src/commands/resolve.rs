use clap::{Args, Subcommand};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;
use serde_json::json;

use crate::config::find_repo_root;
use crate::conflict_writer;
use crate::merge_state;
use crate::worktree;

#[derive(Args)]
pub struct ResolveArgs {
    /// Emit machine-readable JSON output
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: ResolveCommand,
}

#[derive(Subcommand)]
enum ResolveCommand {
    /// List unresolved conflicts
    List,
    /// Mark a conflict as resolved
    Mark {
        /// Path of the conflicted file
        path: String,
    },
    /// Abort the merge and restore to the left revision
    Abort,
}

pub fn run(args: ResolveArgs) -> anyhow::Result<()> {
    match args.command {
        ResolveCommand::List => run_list(args.json),
        ResolveCommand::Mark { path } => run_mark(&path, args.json),
        ResolveCommand::Abort => run_abort(args.json),
    }
}

fn run_list(json_output: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let claw_dir = store.layout().claw_dir();

    if !merge_state::exists(&claw_dir) {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "schema_version": 1,
                    "action": "resolve.list",
                    "merge_in_progress": false,
                    "left_ref": null,
                    "right_ref": null,
                    "left_revision": null,
                    "right_revision": null,
                    "base_revision": null,
                    "conflict_count": 0,
                    "ready_count": 0,
                    "unresolved_count": 0,
                    "conflicts": [],
                }))?
            );
            return Ok(());
        }
        println!("No merge in progress.");
        return Ok(());
    }

    let ms = merge_state::read_from(&claw_dir)?;
    if json_output {
        let conflicts: Vec<_> = ms
            .conflicts
            .iter()
            .map(|conflict| {
                let file_path = root.join(&conflict.file_path);
                let has_markers = check_conflict_markers(&file_path, &conflict.codec_id);
                json!({
                    "path": conflict.file_path,
                    "conflict_id": conflict.conflict_id,
                    "codec": conflict.codec_id,
                    "status": if has_markers { "unresolved" } else { "ready" },
                    "has_markers": has_markers,
                    "reason": conflict.reason,
                    "regions": conflict.regions,
                })
            })
            .collect();
        let ready_count = conflicts
            .iter()
            .filter(|conflict| conflict["status"] == "ready")
            .count();

        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "action": "resolve.list",
                "merge_in_progress": true,
                "left_ref": ms.merge.left_ref,
                "right_ref": ms.merge.right_ref,
                "left_revision": ms.merge.left_revision,
                "right_revision": ms.merge.right_revision,
                "base_revision": ms.merge.base_revision,
                "conflict_count": conflicts.len(),
                "ready_count": ready_count,
                "unresolved_count": conflicts.len().saturating_sub(ready_count),
                "conflicts": conflicts,
            }))?
        );
        return Ok(());
    }

    println!("Merging: {} into {}", ms.merge.right_ref, ms.merge.left_ref);
    println!();

    if ms.conflicts.is_empty() {
        println!("No conflicts. Run `claw snapshot -m <message>` to complete the merge.");
        return Ok(());
    }

    for conflict in &ms.conflicts {
        let file_path = root.join(&conflict.file_path);
        let has_markers = check_conflict_markers(&file_path, &conflict.codec_id);
        let status_tag = if has_markers { "unresolved" } else { "ready" };
        println!(
            "  {} {} ({})",
            status_tag, conflict.file_path, conflict.codec_id
        );
        if let Some(reason) = &conflict.reason {
            println!("    reason: {reason}");
        }
        if !conflict.regions.is_empty() {
            println!("    collided regions:");
            for region in &conflict.regions {
                println!(
                    "      {} {} at {} ({})",
                    region.side, region.op_type, region.address, region.patch_id
                );
            }
        }
    }

    println!();
    println!("  (use \"claw resolve mark <path>\" to mark a file as resolved)");
    println!("  (use \"claw resolve abort\" to cancel the merge)");

    Ok(())
}

fn run_mark(path: &str, json_output: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let claw_dir = store.layout().claw_dir();

    if !merge_state::exists(&claw_dir) {
        anyhow::bail!("No merge in progress.");
    }

    let mut ms = merge_state::read_from(&claw_dir)?;

    let idx = ms
        .conflicts
        .iter()
        .position(|c| c.file_path == path)
        .ok_or_else(|| anyhow::anyhow!("'{}' is not a conflicted file", path))?;

    // Check that conflict markers have been removed
    let file_path = root.join(path);
    let codec_id = &ms.conflicts[idx].codec_id;
    if check_conflict_markers(&file_path, codec_id) {
        anyhow::bail!(
            "File '{}' still contains conflict markers. Edit the file to resolve, then re-run.",
            path
        );
    }

    // Remove conflict sidecars
    let conflicted_path = root.join(path);
    let base_sidecar = conflict_writer::sidecar_path(&conflicted_path, "BASE");
    let right_sidecar = conflict_writer::sidecar_path(&conflicted_path, "RIGHT");
    let _ = std::fs::remove_file(base_sidecar);
    let _ = std::fs::remove_file(right_sidecar);

    // Remove from conflicts list
    ms.conflicts.remove(idx);
    merge_state::write_to(&claw_dir, &ms)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "action": "resolve.mark",
                "path": path,
                "marked": true,
                "remaining_conflict_count": ms.conflicts.len(),
                "merge_complete": ms.conflicts.is_empty(),
            }))?
        );
        return Ok(());
    }

    println!("Marked '{}' as resolved.", path);
    if ms.conflicts.is_empty() {
        println!("All conflicts resolved. Run `claw snapshot -m <message>` to complete the merge.");
    } else {
        println!("{} conflict(s) remaining.", ms.conflicts.len());
    }

    Ok(())
}

fn run_abort(json_output: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let claw_dir = store.layout().claw_dir();

    if !merge_state::exists(&claw_dir) {
        anyhow::bail!("No merge in progress.");
    }

    let ms = merge_state::read_from(&claw_dir)?;

    // Clean up conflict sidecars
    for conflict in &ms.conflicts {
        let conflicted_path = root.join(&conflict.file_path);
        let base_sidecar = conflict_writer::sidecar_path(&conflicted_path, "BASE");
        let right_sidecar = conflict_writer::sidecar_path(&conflicted_path, "RIGHT");
        let _ = std::fs::remove_file(base_sidecar);
        let _ = std::fs::remove_file(right_sidecar);
    }

    // Restore worktree to left revision
    let left_id = ObjectId::from_hex(&ms.merge.left_revision)?;
    let left_obj = store.load_object(&left_id)?;
    if let Object::Revision(ref rev) = left_obj {
        if let Some(ref tree_id) = rev.tree {
            worktree::materialize_tree(&store, tree_id, &root)?;
        }
    }

    // Remove merge state
    merge_state::remove(&claw_dir)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "action": "resolve.abort",
                "aborted": true,
                "restored_ref": ms.merge.left_ref,
                "restored_revision": ms.merge.left_revision,
                "conflict_count": ms.conflicts.len(),
            }))?
        );
        return Ok(());
    }

    println!(
        "Merge aborted. Working tree restored to {}.",
        &ms.merge.left_ref
    );
    Ok(())
}

/// Check if a file still contains conflict markers.
fn check_conflict_markers(path: &std::path::Path, codec_id: &str) -> bool {
    match codec_id {
        "json/tree" => {
            // JSON conflicts use a structured _conflict key
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    return val.get("_conflict").is_some();
                }
            }
            false
        }
        "binary" => {
            // Binary conflicts use sidecars — check if sidecars exist
            conflict_writer::sidecar_path(path, "BASE").exists()
                || conflict_writer::sidecar_path(path, "RIGHT").exists()
        }
        _ => {
            // Text conflicts use <<<<<<< markers
            if let Ok(content) = std::fs::read_to_string(path) {
                content.contains("<<<<<<<") && content.contains(">>>>>>>")
            } else {
                false
            }
        }
    }
}
