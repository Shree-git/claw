use clap::{Args, Subcommand};
use std::path::PathBuf;

use claw_core::object::Object;
use claw_core::types::{Blob, Patch};
use claw_patch::CodecRegistry;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct PatchArgs {
    #[command(subcommand)]
    command: PatchCommand,
}

#[derive(Subcommand)]
enum PatchCommand {
    /// Create a patch from two files
    Create {
        /// Old file
        #[arg(long)]
        old: PathBuf,
        /// New file
        #[arg(long)]
        new: PathBuf,
        /// Target path in repo
        #[arg(short, long)]
        path: String,
    },
    /// Apply a patch to a file
    Apply {
        /// Patch object ID
        #[arg(short, long)]
        patch: String,
        /// File to apply to
        #[arg(short, long)]
        file: PathBuf,
    },
    /// Show a patch
    Show {
        /// Patch object ID
        id: String,
    },
}

pub fn run(args: PatchArgs) -> anyhow::Result<()> {
    match args.command {
        PatchCommand::Create { old, new, path } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();

            let old_data = std::fs::read(&old)?;
            let new_data = std::fs::read(&new)?;

            // Store blobs
            let old_blob = Object::Blob(Blob {
                data: old_data.clone(),
                media_type: None,
            });
            let new_blob = Object::Blob(Blob {
                data: new_data.clone(),
                media_type: None,
            });
            let old_id = store.store_object(&old_blob)?;
            let new_id = store.store_object(&new_blob)?;

            // Determine codec from extension
            let ext = std::path::Path::new(&path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("txt");
            let codec = registry
                .get_by_extension(ext)
                .or_else(|| registry.get("text/line").ok())
                .ok_or_else(|| anyhow::anyhow!("no codec for extension: {ext}"))?;

            let ops = codec.diff(&old_data, &new_data)?;

            let patch = Patch {
                target_path: path.clone(),
                codec_id: codec.id().to_string(),
                base_object: Some(old_id),
                result_object: Some(new_id),
                ops,
                codec_payload: None,
            };

            let patch_id = store.store_object(&Object::Patch(patch))?;
            println!("Created patch: {patch_id}");
            println!("  Path: {path}");
            println!("  Codec: {}", codec.id());
        }
        PatchCommand::Apply { patch, file } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();

            let patch_id = claw_core::id::ObjectId::from_display(&patch)?;
            let obj = store.load_object(&patch_id)?;
            let p = match obj {
                Object::Patch(p) => p,
                _ => anyhow::bail!("object is not a patch"),
            };

            let base_data = std::fs::read(&file)?;
            let codec = registry.get(&p.codec_id)?;
            let result = codec.apply(&base_data, &p.ops)?;

            std::fs::write(&file, &result)?;
            println!("Applied patch to {}", file.display());
        }
        PatchCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;

            let patch_id = claw_core::id::ObjectId::from_display(&id)?;
            let obj = store.load_object(&patch_id)?;
            if let Object::Patch(p) = obj {
                println!("Patch: {id}");
                println!("  Target: {}", p.target_path);
                println!("  Codec: {}", p.codec_id);
                println!("  Ops: {}", p.ops.len());
                for (i, op) in p.ops.iter().enumerate() {
                    println!("    [{i}] {} at {}", op.op_type, op.address);
                }
            }
        }
    }
    Ok(())
}
