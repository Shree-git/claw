use clap::{Args, Subcommand};

use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;

#[derive(Args)]
pub struct BranchArgs {
    /// Output result as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<BranchCommand>,
}

#[derive(Subcommand)]
enum BranchCommand {
    /// Create a new branch
    #[command(alias = "new")]
    Create {
        /// Name of the new branch
        name: String,
        /// Preview without creating the branch ref
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete a branch
    #[command(alias = "rm", alias = "del")]
    Delete {
        /// Name of the branch to delete
        name: String,
        /// Preview without deleting the branch ref
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(args: BranchArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    match args.command {
        None => {
            // List branches
            let head_state = store.read_head()?;
            let current_branch = match &head_state {
                HeadState::Symbolic { ref_name } => Some(ref_name.clone()),
                HeadState::Detached { .. } => None,
            };

            let refs = store.list_refs("heads/")?;
            if args.json {
                let branches = if refs.is_empty() {
                    current_branch
                        .as_ref()
                        .map(|branch| {
                            let name = branch.strip_prefix("heads/").unwrap_or(branch);
                            vec![serde_json::json!({
                                "name": name,
                                "ref": branch,
                                "current": true,
                                "target": serde_json::Value::Null,
                                "unborn": true,
                            })]
                        })
                        .unwrap_or_default()
                } else {
                    refs.iter()
                        .map(|(name, id)| {
                            serde_json::json!({
                                "name": name.strip_prefix("heads/").unwrap_or(name),
                                "ref": name,
                                "current": current_branch.as_deref() == Some(name.as_str()),
                                "target": id.to_string(),
                                "unborn": false,
                            })
                        })
                        .collect()
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "branch.list",
                        "current": current_branch
                            .as_deref()
                            .map(|name| name.strip_prefix("heads/").unwrap_or(name)),
                        "branch_count": branches.len(),
                        "branches": branches,
                    }))?
                );
            } else if refs.is_empty() {
                // Show default branch even if no refs exist
                if let Some(ref branch) = current_branch {
                    let name = branch.strip_prefix("heads/").unwrap_or(branch);
                    println!("* {} (no commits yet)", name);
                }
            } else {
                for (name, _id) in &refs {
                    let is_current = current_branch.as_deref() == Some(name.as_str());
                    let marker = if is_current { "* " } else { "  " };
                    let short_name = name.strip_prefix("heads/").unwrap_or(name);
                    println!("{}{}", marker, short_name);
                }
            }
        }
        Some(BranchCommand::Create { name, dry_run }) => {
            let head_id = store
                .resolve_head()?
                .ok_or_else(|| anyhow::anyhow!("no commits yet; create a snapshot first with `claw snapshot -m \"initial snapshot\"`"))?;
            let ref_name = format!("heads/{}", name);
            if store.get_ref(&ref_name)?.is_some() {
                anyhow::bail!(
                    "branch '{}' already exists. Run `claw checkout {}` to switch to it.",
                    name,
                    name
                );
            }
            if !dry_run {
                store.set_ref(&ref_name, &head_id)?;
            }
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "branch.create",
                        "branch": name,
                        "ref": ref_name,
                        "target": head_id.to_string(),
                        "dry_run": dry_run,
                        "created": !dry_run,
                    }))?
                );
            } else if dry_run {
                println!("Would create branch '{}' at {}", name, head_id);
            } else {
                println!("Created branch '{}' at {}", name, head_id);
            }
        }
        Some(BranchCommand::Delete { name, dry_run }) => {
            let head_state = store.read_head()?;
            let ref_name = format!("heads/{}", name);
            if let HeadState::Symbolic { ref_name: current } = &head_state {
                if current == &ref_name {
                    anyhow::bail!(
                        "cannot delete the current branch '{}'. Checkout another branch first.",
                        name
                    );
                }
            }
            let target = store.get_ref(&ref_name)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "branch '{}' not found. Run `claw branch` to list available branches.",
                    name
                )
            })?;
            if !dry_run {
                store.delete_ref(&ref_name)?;
            }
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "branch.delete",
                        "branch": name,
                        "ref": ref_name,
                        "target": target.to_string(),
                        "dry_run": dry_run,
                        "deleted": !dry_run,
                    }))?
                );
            } else if dry_run {
                println!("Would delete branch '{}'", name);
            } else {
                println!("Deleted branch '{}'", name);
            }
        }
    }

    Ok(())
}
