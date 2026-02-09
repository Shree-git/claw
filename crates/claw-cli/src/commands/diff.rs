use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_patch::CodecRegistry;
use claw_store::tree_diff::{diff_trees, ChangeKind};
use claw_store::ClawStore;

use crate::config::find_repo_root;
use crate::diff_render;
use crate::ignore::IgnoreRules;
use crate::worktree;

#[derive(Args)]
pub struct DiffArgs {
    /// Source ref (default: HEAD)
    #[arg(long)]
    from: Option<String>,
    /// Target ref (default: working tree)
    #[arg(long)]
    to: Option<String>,
    /// Filter by path
    #[arg(long)]
    path: Option<String>,
    /// Show only changed file names
    #[arg(long)]
    name_only: bool,
}

pub fn run(args: DiffArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let registry = CodecRegistry::default();

    let from_tree = resolve_tree(&store, args.from.as_deref(), true)?;
    let to_tree = if args.to.is_some() {
        resolve_tree(&store, args.to.as_deref(), false)?
    } else {
        // Working tree
        let ignore = IgnoreRules::load(&root);
        Some(worktree::scan_worktree(&store, &root, &ignore)?)
    };

    let changes = diff_trees(&store, from_tree.as_ref(), to_tree.as_ref(), "")?;

    // Filter by path prefix if specified
    let changes: Vec<_> = if let Some(ref filter_path) = args.path {
        changes
            .into_iter()
            .filter(|c| c.path.starts_with(filter_path.as_str()))
            .collect()
    } else {
        changes
    };

    if changes.is_empty() {
        return Ok(());
    }

    let mut sorted = changes;
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    for change in &sorted {
        if args.name_only {
            let tag = match change.kind {
                ChangeKind::Added => "A",
                ChangeKind::Deleted => "D",
                ChangeKind::Modified => "M",
                ChangeKind::TypeChanged => "T",
            };
            println!("{} {}", tag, change.path);
            continue;
        }

        let ext = change.path.rsplit('.').next().unwrap_or("");
        let codec = registry.get_by_extension(ext);

        let old_bytes = load_blob_data(&store, change.old_id.as_ref());
        let new_bytes = load_blob_data(&store, change.new_id.as_ref());

        if let Some(codec) = codec {
            if codec.id().starts_with("json") {
                // JSON diff — fall back to unified text if parsing fails (e.g. added/deleted files)
                match codec.diff(&old_bytes, &new_bytes) {
                    Ok(ops) => print!("{}", diff_render::render_json_diff(&change.path, &ops)),
                    Err(_) => print!("{}", diff_render::render_unified_diff(&change.path, &old_bytes, &new_bytes)),
                }
            } else {
                // Text diff
                print!("{}", diff_render::render_unified_diff(&change.path, &old_bytes, &new_bytes));
            }
        } else {
            // Binary or unknown
            let old_hash = change.old_id.map(|id| id.to_hex()).unwrap_or_default();
            let new_hash = change.new_id.map(|id| id.to_hex()).unwrap_or_default();
            print!(
                "{}",
                diff_render::render_binary_diff(
                    &change.path,
                    old_bytes.len(),
                    new_bytes.len(),
                    &old_hash,
                    &new_hash,
                )
            );
        }
    }

    Ok(())
}

fn resolve_tree(store: &ClawStore, ref_name: Option<&str>, default_head: bool) -> anyhow::Result<Option<ObjectId>> {
    let ref_name = match ref_name {
        Some(r) => r,
        None if default_head => {
            let head_id = store.resolve_head()?;
            return match head_id {
                Some(id) => {
                    let obj = store.load_object(&id)?;
                    match obj {
                        Object::Revision(rev) => Ok(rev.tree),
                        _ => Ok(None),
                    }
                }
                None => Ok(None),
            };
        }
        None => return Ok(None),
    };

    // Try as ref first
    let id = if let Some(id) = store.get_ref(ref_name)? {
        id
    } else if let Ok(id) = ObjectId::from_hex(ref_name) {
        id
    } else {
        anyhow::bail!("cannot resolve: {}", ref_name);
    };

    let obj = store.load_object(&id)?;
    match obj {
        Object::Revision(rev) => Ok(rev.tree),
        Object::Tree(_) => Ok(Some(id)),
        _ => anyhow::bail!("not a revision or tree: {}", ref_name),
    }
}

fn load_blob_data(store: &ClawStore, id: Option<&ObjectId>) -> Vec<u8> {
    match id {
        Some(id) => match store.load_object(id) {
            Ok(Object::Blob(b)) => b.data,
            _ => vec![],
        },
        None => vec![],
    }
}
