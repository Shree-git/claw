use clap::Args;

use claw_core::object::Object;
use claw_merge::emit::merge;
use claw_patch::CodecRegistry;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct IntegrateArgs {
    /// Left ref (e.g., heads/main)
    #[arg(long)]
    left: String,
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

    let left_id = store
        .get_ref(&args.left)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.left))?;
    let right_id = store
        .get_ref(&args.right)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.right))?;

    let result = merge(&store, &registry, &left_id, &right_id, &args.author, &args.message)?;

    let rev_id = store.store_object(&Object::Revision(result.revision))?;
    store.set_ref(&args.left, &rev_id)?;

    if result.conflicts.is_empty() {
        println!("Integrated successfully: {rev_id}");
    } else {
        println!("Integrated with {} conflict(s): {rev_id}", result.conflicts.len());
        for c in &result.conflicts {
            println!("  Conflict in {} ({})", c.file_path, c.codec_id);
        }
    }

    Ok(())
}
