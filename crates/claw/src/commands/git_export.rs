use clap::Args;
use std::path::{Component, Path};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_git::exporter::GitExporter;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct GitExportArgs {
    /// Ref to export (default: heads/main)
    #[arg(long, name = "ref", default_value = "heads/main")]
    ref_name: String,
    /// Git branch name to create
    #[arg(long, default_value = "claw/main")]
    branch: String,
    /// Path to .git directory
    #[arg(long, default_value = ".git")]
    git_dir: String,
}

fn validate_git_branch_path(branch: &str) -> anyhow::Result<()> {
    let path = Path::new(branch);
    if path.is_absolute() {
        anyhow::bail!("invalid branch name '{}': must be relative", branch);
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                anyhow::bail!(
                    "invalid branch name '{}': cannot contain '.', '..', or root components",
                    branch
                );
            }
        }
    }

    Ok(())
}

pub fn run(args: GitExportArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    let rev_id = store
        .get_ref(&args.ref_name)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.ref_name))?;

    let git_dir = root.join(&args.git_dir);
    let git_objects_dir = git_dir.join("objects");

    let mut exporter = GitExporter::new(&store);
    let head_sha1 = exporter.export(&rev_id, &git_objects_dir)?;

    // Write git branch ref
    validate_git_branch_path(&args.branch)?;
    let refs_dir = git_dir.join("refs").join("heads");
    std::fs::create_dir_all(&refs_dir)?;
    let branch_path = refs_dir.join(&args.branch);
    if let Some(parent) = branch_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(branch_path, format!("{}\n", hex::encode(head_sha1)))?;

    // Walk DAG and write change mapping refs
    write_change_refs(&store, &exporter, &rev_id, &git_dir)?;

    println!("Exported to git: refs/heads/{}", args.branch);
    println!("  SHA-1: {}", hex::encode(head_sha1));

    Ok(())
}

fn write_change_refs(
    store: &ClawStore,
    exporter: &GitExporter,
    start: &ObjectId,
    git_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let refs_dir = git_dir.join("refs").join("claw").join("changes");
    std::fs::create_dir_all(&refs_dir)?;

    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![*start];

    while let Some(id) = queue.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Ok(Object::Revision(ref rev)) = store.load_object(&id) {
            if let (Some(change_id), Some(sha1)) = (rev.change_id.as_ref(), exporter.get_sha1(&id))
            {
                std::fs::write(
                    refs_dir.join(change_id.to_string()),
                    format!("{}\n", hex::encode(sha1)),
                )?;
            }
            queue.extend_from_slice(&rev.parents);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_git_branch_path;

    #[test]
    fn allows_relative_branch_paths() {
        assert!(validate_git_branch_path("main").is_ok());
        assert!(validate_git_branch_path("claw/main").is_ok());
    }

    #[test]
    fn rejects_parent_and_root_components() {
        assert!(validate_git_branch_path("../outside").is_err());
        assert!(validate_git_branch_path("claw/../outside").is_err());
        assert!(validate_git_branch_path("/absolute").is_err());
    }
}
