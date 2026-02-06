use clap::{Args, Subcommand};

use claw_core::id::{ChangeId, IntentId};
use claw_core::object::Object;
use claw_core::types::{Change, ChangeStatus};
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct ChangeArgs {
    #[command(subcommand)]
    command: ChangeCommand,
}

#[derive(Subcommand)]
enum ChangeCommand {
    /// Create a new change
    New {
        /// Intent ID this change belongs to
        #[arg(short, long)]
        intent: String,
    },
    /// Show a change
    Show {
        /// Change ID (ULID)
        id: String,
    },
    /// List changes
    List {
        /// Filter by intent ID
        #[arg(short, long)]
        intent: Option<String>,
    },
    /// Update change status
    Status {
        /// Change ID
        id: String,
        /// New status
        status: String,
    },
}

pub fn run(args: ChangeArgs) -> anyhow::Result<()> {
    match args.command {
        ChangeCommand::New {
            intent,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64;

            let intent_id = IntentId::from_string(&intent)?;
            let change = Change {
                id: ChangeId::new(),
                intent_id,
                head_revision: None,
                workstream_id: None,
                status: ChangeStatus::Open,
                created_at_ms: now,
                updated_at_ms: now,
            };

            let id = store.store_object(&Object::Change(change.clone()))?;
            store.set_ref(&format!("changes/{}", change.id), &id)?;

            println!("Created change: {}", change.id);
            println!("  Intent: {intent}");
            println!("  Object: {id}");
        }
        ChangeCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store
                .get_ref(&format!("changes/{id}"))?
                .ok_or_else(|| anyhow::anyhow!("change not found: {id}"))?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Change(change) = obj {
                println!("Change: {}", change.id);
                println!("  Intent: {}", change.intent_id);
                println!("  Status: {:?}", change.status);
                println!("  Head revision: {:?}", change.head_revision);
            }
        }
        ChangeCommand::List { intent } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("changes")?;
            for (_, id) in &refs {
                if let Ok(obj) = store.load_object(id) {
                    if let Object::Change(change) = obj {
                        if let Some(ref filter) = intent {
                            if change.intent_id.to_string() != *filter {
                                continue;
                            }
                        }
                        println!(
                            "{} {:?} intent:{}",
                            change.id, change.status, change.intent_id
                        );
                    }
                }
            }
        }
        ChangeCommand::Status { id, status } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store
                .get_ref(&format!("changes/{id}"))?
                .ok_or_else(|| anyhow::anyhow!("change not found: {id}"))?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Change(mut change) = obj {
                change.status = match status.to_lowercase().as_str() {
                    "open" => ChangeStatus::Open,
                    "ready" => ChangeStatus::Ready,
                    "integrated" => ChangeStatus::Integrated,
                    "abandoned" => ChangeStatus::Abandoned,
                    _ => anyhow::bail!("unknown status: {status}"),
                };
                change.updated_at_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis() as u64;
                let new_id = store.store_object(&Object::Change(change.clone()))?;
                store.set_ref(&format!("changes/{}", change.id), &new_id)?;
                println!("Updated change {} to {:?}", change.id, change.status);
            }
        }
    }
    Ok(())
}
