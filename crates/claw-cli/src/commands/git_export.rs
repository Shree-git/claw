use clap::Args;

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
    let refs_dir = git_dir.join("refs").join("heads");
    std::fs::create_dir_all(&refs_dir)?;
    std::fs::write(refs_dir.join(&args.branch), format!("{}\n", hex::encode(head_sha1)))?;

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
        if let Ok(obj) = store.load_object(&id) {
            if let Object::Revision(ref rev) = obj {
                if let Some(ref change_id) = rev.change_id {
                    if let Some(sha1) = exporter.get_sha1(&id) {
                        std::fs::write(
                            refs_dir.join(change_id.to_string()),
                            format!("{}\n", hex::encode(sha1)),
                        )?;
                    }
                }
                queue.extend_from_slice(&rev.parents);
            }
        }
    }

    Ok(())
}
