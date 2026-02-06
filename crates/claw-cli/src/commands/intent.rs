use clap::{Args, Subcommand};

use claw_core::id::IntentId;
use claw_core::object::Object;
use claw_core::types::{Intent, IntentStatus};
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct IntentArgs {
    #[command(subcommand)]
    command: IntentCommand,
}

#[derive(Subcommand)]
enum IntentCommand {
    /// Create a new intent
    New {
        /// Intent title
        #[arg(short, long)]
        title: String,
        /// Intent goal
        #[arg(short, long, default_value = "")]
        goal: String,
    },
    /// Show an intent
    Show {
        /// Intent ID (ULID)
        id: String,
    },
    /// List intents
    List,
    /// Update an intent
    Update {
        /// Intent ID (ULID)
        id: String,
        /// New status
        #[arg(short, long)]
        status: Option<String>,
    },
}

pub fn run(args: IntentArgs) -> anyhow::Result<()> {
    match args.command {
        IntentCommand::New {
            title,
            goal,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64;

            let intent = Intent {
                id: IntentId::new(),
                title: title.clone(),
                goal,
                constraints: vec![],
                acceptance_tests: vec![],
                links: vec![],
                policy_refs: vec![],
                agents: vec![],
                change_ids: vec![],
                depends_on: vec![],
                supersedes: vec![],
                status: IntentStatus::Open,
                created_at_ms: now,
                updated_at_ms: now,
            };

            let id = store.store_object(&Object::Intent(intent.clone()))?;
            store.set_ref(&format!("intents/{}", intent.id), &id)?;

            println!("Created intent: {}", intent.id);
            println!("  Title: {title}");
            println!("  Object: {id}");
        }
        IntentCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store
                .get_ref(&format!("intents/{id}"))?
                .ok_or_else(|| anyhow::anyhow!("intent not found: {id}"))?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Intent(intent) = obj {
                println!("Intent: {}", intent.id);
                println!("  Title: {}", intent.title);
                println!("  Status: {:?}", intent.status);
                println!("  Goal: {}", intent.goal);
            }
        }
        IntentCommand::List => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("intents")?;
            if refs.is_empty() {
                println!("No intents found.");
            } else {
                for (_name, id) in &refs {
                    if let Ok(obj) = store.load_object(id) {
                        if let Object::Intent(intent) = obj {
                            println!(
                                "{} {:?} {}",
                                intent.id, intent.status, intent.title
                            );
                        }
                    }
                }
            }
        }
        IntentCommand::Update { id, status } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store
                .get_ref(&format!("intents/{id}"))?
                .ok_or_else(|| anyhow::anyhow!("intent not found: {id}"))?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Intent(mut intent) = obj {
                if let Some(s) = status {
                    intent.status = match s.to_lowercase().as_str() {
                        "open" => IntentStatus::Open,
                        "blocked" => IntentStatus::Blocked,
                        "done" => IntentStatus::Done,
                        "superseded" => IntentStatus::Superseded,
                        _ => anyhow::bail!("unknown status: {s}"),
                    };
                }
                intent.updated_at_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis() as u64;
                let new_id = store.store_object(&Object::Intent(intent.clone()))?;
                store.set_ref(&format!("intents/{}", intent.id), &new_id)?;
                println!("Updated intent: {}", intent.id);
            }
        }
    }
    Ok(())
}
