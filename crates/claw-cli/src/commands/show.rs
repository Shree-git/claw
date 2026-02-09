use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::FileMode;
use claw_store::ClawStore;

use crate::config::find_repo_root;
use crate::output;

#[derive(Args)]
pub struct ShowArgs {
    /// Object ID (hex or clw_ display format), or ref name
    object: String,
}

pub fn run(args: ShowArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    // Resolve: try ref first, then hex, then display format
    let id = if let Some(id) = store.get_ref(&args.object)? {
        id
    } else if let Ok(id) = ObjectId::from_hex(&args.object) {
        if store.has_object(&id) {
            id
        } else {
            anyhow::bail!("object not found: {}", args.object);
        }
    } else if let Ok(id) = ObjectId::from_display(&args.object) {
        if store.has_object(&id) {
            id
        } else {
            anyhow::bail!("object not found: {}", args.object);
        }
    } else {
        anyhow::bail!("cannot resolve: {}", args.object);
    };

    let obj = store.load_object(&id)?;
    let type_name = obj.type_tag().name();

    println!("{}", output::header(&format!("{} {}", type_name, id)));
    println!("{}", output::kv("hex", &id.to_hex()));
    println!();

    match obj {
        Object::Revision(rev) => {
            if !rev.parents.is_empty() {
                let parents: Vec<String> = rev.parents.iter().map(|p| p.to_string()).collect();
                println!("{}", output::kv("parents", &parents.join(", ")));
            }
            if let Some(tree) = rev.tree {
                println!("{}", output::kv("tree", &tree.to_string()));
            }
            if !rev.patches.is_empty() {
                println!(
                    "{}",
                    output::kv("patches", &format!("{} patch(es)", rev.patches.len()))
                );
                for p in &rev.patches {
                    println!("                  {}", p);
                }
            }
            println!("{}", output::kv("author", &rev.author));
            println!(
                "{}",
                output::kv("date", &format_timestamp(rev.created_at_ms))
            );
            if let Some(ref cid) = rev.change_id {
                println!("{}", output::kv("change_id", &cid.to_string()));
            }
            if let Some(ref cap) = rev.capsule_id {
                println!("{}", output::kv("capsule", &cap.to_string()));
            }
            println!();
            println!("    {}", rev.summary);
        }
        Object::Tree(tree) => {
            println!("{} entries:", tree.entries.len());
            println!();
            for entry in &tree.entries {
                let mode_str = match entry.mode {
                    FileMode::Regular => "file",
                    FileMode::Executable => "exec",
                    FileMode::Symlink => "link",
                    FileMode::Directory => "dir ",
                };
                println!("  {} {} {}", mode_str, entry.object_id, entry.name);
            }
        }
        Object::Blob(blob) => {
            if let Ok(text) = std::str::from_utf8(&blob.data) {
                if blob.data.len() <= 8192 {
                    println!("{}", text);
                } else {
                    println!("(text, {} bytes, showing first 8192)", blob.data.len());
                    println!("{}", &text[..8192]);
                }
            } else {
                println!("(binary, {} bytes)", blob.data.len());
            }
        }
        Object::Patch(patch) => {
            println!("{}", output::kv("target", &patch.target_path));
            println!("{}", output::kv("codec", &patch.codec_id));
            if let Some(ref base) = patch.base_object {
                println!("{}", output::kv("base", &base.to_string()));
            }
            if let Some(ref result) = patch.result_object {
                println!("{}", output::kv("result", &result.to_string()));
            }
            println!(
                "{}",
                output::kv("ops", &format!("{} operation(s)", patch.ops.len()))
            );
            for op in &patch.ops {
                println!("  {} @ {}", op.op_type, op.address);
            }
        }
        Object::Intent(intent) => {
            println!("{}", output::kv("id", &intent.id.to_string()));
            println!("{}", output::kv("title", &intent.title));
            println!("{}", output::kv("status", &format!("{:?}", intent.status)));
            println!("{}", output::kv("goal", &intent.goal));
            if !intent.constraints.is_empty() {
                println!(
                    "{}",
                    output::kv("constraints", &intent.constraints.join(", "))
                );
            }
            if !intent.agents.is_empty() {
                println!("{}", output::kv("agents", &intent.agents.join(", ")));
            }
        }
        Object::Change(change) => {
            println!("{}", output::kv("id", &change.id.to_string()));
            println!("{}", output::kv("intent", &change.intent_id.to_string()));
            println!("{}", output::kv("status", &format!("{:?}", change.status)));
            if let Some(ref head) = change.head_revision {
                println!("{}", output::kv("head_revision", &head.to_string()));
            }
        }
        Object::Capsule(capsule) => {
            println!(
                "{}",
                output::kv("revision", &capsule.revision_id.to_string())
            );
            println!(
                "{}",
                output::kv("agent_id", &capsule.public_fields.agent_id)
            );
            if let Some(ref ver) = capsule.public_fields.agent_version {
                println!("{}", output::kv("agent_version", ver));
            }
            if !capsule.public_fields.evidence.is_empty() {
                println!(
                    "{}",
                    output::kv(
                        "evidence",
                        &format!("{} item(s)", capsule.public_fields.evidence.len())
                    )
                );
                for e in &capsule.public_fields.evidence {
                    println!("  {} ({})", e.name, e.status);
                }
            }
            println!(
                "{}",
                output::kv("signatures", &format!("{}", capsule.signatures.len()))
            );
            if capsule.encrypted_private.is_some() {
                println!("{}", output::kv("private", "encrypted"));
            }
        }
        Object::Snapshot(snap) => {
            println!("{}", output::kv("revision", &snap.revision_id.to_string()));
            println!("{}", output::kv("tree_root", &snap.tree_root.to_string()));
            println!(
                "{}",
                output::kv("date", &format_timestamp(snap.created_at_ms))
            );
        }
        Object::Conflict(conflict) => {
            println!("{}", output::kv("file_path", &conflict.file_path));
            println!("{}", output::kv("codec", &conflict.codec_id));
            println!(
                "{}",
                output::kv("status", &format!("{:?}", conflict.status))
            );
            println!(
                "{}",
                output::kv("left", &conflict.left_revision.to_string())
            );
            println!(
                "{}",
                output::kv("right", &conflict.right_revision.to_string())
            );
        }
        Object::Policy(policy) => {
            println!("{}", output::kv("policy_id", &policy.policy_id));
            println!(
                "{}",
                output::kv("visibility", &format!("{:?}", policy.visibility))
            );
            if !policy.required_checks.is_empty() {
                println!(
                    "{}",
                    output::kv("checks", &policy.required_checks.join(", "))
                );
            }
        }
        Object::Workstream(ws) => {
            println!("{}", output::kv("id", &ws.workstream_id));
            println!(
                "{}",
                output::kv("changes", &format!("{}", ws.change_stack.len()))
            );
        }
        Object::RefLog(reflog) => {
            println!("{}", output::kv("ref", &reflog.ref_name));
            println!(
                "{}",
                output::kv("entries", &format!("{}", reflog.entries.len()))
            );
            for entry in &reflog.entries {
                let old = entry
                    .old_target
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "(none)".to_string());
                println!(
                    "  {} -> {} [{}] {}",
                    old, entry.new_target, entry.author, entry.message
                );
            }
        }
    }

    println!();
    Ok(())
}

fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let d = secs / 86400;
    format!("{:02}:{:02}:{:02} UTC (day {})", h, m, s, d)
}
