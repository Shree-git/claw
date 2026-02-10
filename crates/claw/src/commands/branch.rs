use clap::{Args, Subcommand};

use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;

#[derive(Args)]
pub struct BranchArgs {
    #[command(subcommand)]
    command: Option<BranchCommand>,
}

#[derive(Subcommand)]
enum BranchCommand {
    /// Create a new branch
    Create {
        /// Name of the new branch
        name: String,
    },
    /// Delete a branch
    Delete {
        /// Name of the branch to delete
        name: String,
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
            if refs.is_empty() {
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
        Some(BranchCommand::Create { name }) => {
            let head_id = store
                .resolve_head()?
                .ok_or_else(|| anyhow::anyhow!("no commits yet; create a snapshot first"))?;
            let ref_name = format!("heads/{}", name);
            if store.get_ref(&ref_name)?.is_some() {
                anyhow::bail!("branch '{}' already exists", name);
            }
            store.set_ref(&ref_name, &head_id)?;
            println!("Created branch '{}' at {}", name, head_id);
        }
        Some(BranchCommand::Delete { name }) => {
            let head_state = store.read_head()?;
            let ref_name = format!("heads/{}", name);
            if let HeadState::Symbolic { ref_name: current } = &head_state {
                if current == &ref_name {
                    anyhow::bail!("cannot delete the current branch '{}'", name);
                }
            }
            if store.get_ref(&ref_name)?.is_none() {
                anyhow::bail!("branch '{}' not found", name);
            }
            store.delete_ref(&ref_name)?;
            println!("Deleted branch '{}'", name);
        }
    }

    Ok(())
}
