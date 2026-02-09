use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::{ClawStore, HeadState};

use crate::config::find_repo_root;

#[derive(Args)]
pub struct LogArgs {
    /// Ref to start from (default: HEAD)
    #[arg(long, name = "ref")]
    ref_name: Option<String>,
    /// Maximum number of entries
    #[arg(long, default_value = "20")]
    limit: usize,
    /// Output as JSON
    #[arg(long)]
    json: bool,
    /// Show all branches
    #[arg(long)]
    all: bool,
}

pub fn run(args: LogArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    let mut tips: Vec<(ObjectId, Option<String>)> = Vec::new();

    if args.all {
        let refs = store.list_refs("heads/")?;
        for (name, id) in refs {
            tips.push((id, Some(name)));
        }
    } else if let Some(ref ref_name) = args.ref_name {
        let id = store
            .get_ref(ref_name)?
            .ok_or_else(|| anyhow::anyhow!("ref not found: {}", ref_name))?;
        tips.push((id, Some(ref_name.clone())));
    } else {
        // Default: HEAD
        let head = store.read_head()?;
        match head {
            HeadState::Symbolic { ref_name } => {
                if let Some(id) = store.get_ref(&ref_name)? {
                    tips.push((id, Some(ref_name)));
                } else {
                    println!("No commits yet.");
                    return Ok(());
                }
            }
            HeadState::Detached { target } => {
                tips.push((target, None));
            }
        }
    }

    if tips.is_empty() {
        println!("No commits yet.");
        return Ok(());
    }

    // Collect revisions from all tips, walking first-parent
    let mut entries: Vec<LogEntry> = Vec::new();
    let mut visited = std::collections::HashSet::new();

    for (tip_id, _branch) in &tips {
        walk_log(&store, tip_id, &mut entries, &mut visited, args.limit * 2)?;
    }

    // Sort by timestamp descending
    entries.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    entries.truncate(args.limit);

    if args.json {
        let json_entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                let mut obj = serde_json::json!({
                    "revision_id": e.revision_id.to_hex(),
                    "author": e.author,
                    "created_at_ms": e.created_at_ms,
                    "summary": e.summary,
                    "parents": e.parents.iter().map(|p| p.to_hex()).collect::<Vec<_>>(),
                });
                if let Some(ref cid) = e.change_id {
                    obj["change_id"] = serde_json::Value::String(cid.clone());
                }
                if let Some(ref iid) = e.intent_title {
                    obj["intent_title"] = serde_json::Value::String(iid.clone());
                }
                if let Some(ref cap) = e.capsule_id {
                    obj["capsule_id"] = serde_json::Value::String(cap.clone());
                }
                obj
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_entries)?);
    } else {
        for entry in &entries {
            println!("revision {}", entry.revision_id);
            if entry.parents.len() > 1 {
                let parent_strs: Vec<String> = entry.parents.iter().map(|p| p.to_hex()[..12].to_string()).collect();
                println!("Merge: {}", parent_strs.join(" "));
            }
            println!("Author: {}", entry.author);
            // Format timestamp
            let secs = entry.created_at_ms / 1000;
            println!("Date:   {} (unix {})", format_timestamp(secs), secs);
            if let Some(ref change) = entry.change_id {
                println!("Change: {}", change);
            }
            if let Some(ref title) = entry.intent_title {
                println!("Intent: {}", title);
            }
            if let Some(ref cap) = entry.capsule_id {
                println!("Capsule: {}", cap);
            }
            println!();
            println!("    {}", entry.summary);
            println!();
        }
    }

    Ok(())
}

struct LogEntry {
    revision_id: ObjectId,
    author: String,
    created_at_ms: u64,
    summary: String,
    parents: Vec<ObjectId>,
    change_id: Option<String>,
    intent_title: Option<String>,
    capsule_id: Option<String>,
}

fn walk_log(
    store: &ClawStore,
    start: &ObjectId,
    entries: &mut Vec<LogEntry>,
    visited: &mut std::collections::HashSet<ObjectId>,
    limit: usize,
) -> anyhow::Result<()> {
    let mut current = Some(*start);

    while let Some(id) = current {
        if entries.len() >= limit || !visited.insert(id) {
            break;
        }

        let obj = store.load_object(&id)?;
        let rev = match obj {
            Object::Revision(r) => r,
            _ => break,
        };

        let change_id = rev.change_id.as_ref().map(|c| c.to_string());

        // Try to find intent title
        let intent_title = if let Some(ref cid) = rev.change_id {
            find_intent_title(store, cid)
        } else {
            None
        };

        // Check for capsule reverse-mapping
        let capsule_id = {
            let prefix = &id.to_hex()[..16];
            store
                .get_ref(&format!("capsules/by-revision/{}", prefix))?
                .map(|cap_id| cap_id.to_string())
        };

        let first_parent = rev.parents.first().copied();

        entries.push(LogEntry {
            revision_id: id,
            author: rev.author,
            created_at_ms: rev.created_at_ms,
            summary: rev.summary,
            parents: rev.parents,
            change_id,
            intent_title,
            capsule_id,
        });

        current = first_parent;
    }

    Ok(())
}

fn find_intent_title(store: &ClawStore, change_id: &claw_core::id::ChangeId) -> Option<String> {
    let change_ref = format!("changes/{}", change_id);
    let change_obj_id = store.get_ref(&change_ref).ok()??;
    let change_obj = store.load_object(&change_obj_id).ok()?;
    if let Object::Change(c) = change_obj {
        let intent_ref = format!("intents/{}", c.intent_id);
        let intent_obj_id = store.get_ref(&intent_ref).ok()??;
        let intent_obj = store.load_object(&intent_obj_id).ok()?;
        if let Object::Intent(i) = intent_obj {
            return Some(i.title);
        }
    }
    None
}

fn format_timestamp(secs: u64) -> String {
    // Simple UTC timestamp
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    // Approximate date from epoch days
    // Simple: just show epoch seconds since we don't have chrono
    format!("{:02}:{:02}:{:02} UTC (day {})", h, m, s, d)
}
