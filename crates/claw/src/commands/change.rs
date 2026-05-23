use clap::{Args, Subcommand};

use claw_core::id::{ChangeId, IntentId};
use claw_core::object::Object;
use claw_core::types::{Change, ChangeStatus};
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct ChangeArgs {
    /// Output result as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: ChangeCommand,
}

#[derive(Subcommand)]
enum ChangeCommand {
    /// Create a new change
    #[command(alias = "create")]
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use clap::Parser;

    use super::{ChangeArgs, ChangeCommand};

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: ChangeArgs,
    }

    #[test]
    fn parse_create_alias_as_new() {
        let cli = TestCli::parse_from(["claw", "create", "--intent", "01J00000000000000000000000"]);

        match cli.args.command {
            ChangeCommand::New { intent } => {
                assert_eq!(intent, "01J00000000000000000000000");
            }
            _ => panic!("expected new command"),
        }
    }
}

pub fn run(args: ChangeArgs) -> anyhow::Result<()> {
    let json = args.json;
    match args.command {
        ChangeCommand::New { intent } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64;

            let intent_id = IntentId::from_string(&intent)?;
            let intent_ref = format!("intents/{intent_id}");
            let intent_obj_id = store
                .get_ref(&intent_ref)?
                .ok_or_else(|| anyhow::anyhow!("intent not found: {intent_id}. Run `claw intent list` to inspect available intents."))?;
            let intent_obj = store.load_object(&intent_obj_id)?;
            let mut intent_obj = match intent_obj {
                Object::Intent(intent) => intent,
                _ => anyhow::bail!("ref does not point to an intent object: {intent_ref}"),
            };

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

            let change_id_string = change.id.to_string();
            if !intent_obj
                .change_ids
                .iter()
                .any(|existing| existing == &change_id_string)
            {
                intent_obj.change_ids.push(change_id_string);
                intent_obj.updated_at_ms = now;
                let new_intent_obj_id = store.store_object(&Object::Intent(intent_obj))?;
                store.set_ref(&intent_ref, &new_intent_obj_id)?;
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "change.create",
                        "created": true,
                        "change": change_json(&change, Some(id.to_hex())),
                    }))?
                );
            } else {
                println!("Created change: {}", change.id);
                println!("  Intent: {intent}");
                println!("  Object: {id}");
            }
        }
        ChangeCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store.get_ref(&format!("changes/{id}"))?.ok_or_else(|| {
                anyhow::anyhow!(
                    "change not found: {id}. Run `claw change list` to inspect available changes."
                )
            })?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Change(change) = obj {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "change.show",
                            "change": change_json(&change, Some(obj_id.to_hex())),
                        }))?
                    );
                } else {
                    println!("Change: {}", change.id);
                    println!("  Intent: {}", change.intent_id);
                    println!("  Status: {:?}", change.status);
                    println!("  Head revision: {:?}", change.head_revision);
                }
            }
        }
        ChangeCommand::List { intent } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("changes")?;
            if json {
                let mut changes = Vec::new();
                for (_, id) in &refs {
                    if let Ok(Object::Change(change)) = store.load_object(id) {
                        let matches_intent = intent
                            .as_ref()
                            .map(|filter| change.intent_id.to_string() == *filter)
                            .unwrap_or(true);
                        if matches_intent {
                            changes.push(change_json(&change, Some(id.to_hex())));
                        }
                    }
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "change.list",
                        "change_count": changes.len(),
                        "filters": {
                            "intent": intent,
                        },
                        "changes": changes,
                    }))?
                );
                return Ok(());
            }
            for (_, id) in &refs {
                if let Ok(Object::Change(change)) = store.load_object(id) {
                    let matches_intent = intent
                        .as_ref()
                        .map(|filter| change.intent_id.to_string() == *filter)
                        .unwrap_or(true);
                    if !matches_intent {
                        continue;
                    }
                    println!(
                        "{} {:?} intent:{}",
                        change.id, change.status, change.intent_id
                    );
                }
            }
        }
        ChangeCommand::Status { id, status } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store.get_ref(&format!("changes/{id}"))?.ok_or_else(|| {
                anyhow::anyhow!(
                    "change not found: {id}. Run `claw change list` to inspect available changes."
                )
            })?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Change(mut change) = obj {
                change.status = match status.to_lowercase().as_str() {
                    "open" => ChangeStatus::Open,
                    "ready" => ChangeStatus::Ready,
                    "integrated" => ChangeStatus::Integrated,
                    "abandoned" => ChangeStatus::Abandoned,
                    _ => anyhow::bail!(
                        "unknown status: {status}. Expected one of: open, ready, integrated, abandoned."
                    ),
                };
                change.updated_at_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis() as u64;
                let new_id = store.store_object(&Object::Change(change.clone()))?;
                store.set_ref(&format!("changes/{}", change.id), &new_id)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "change.status",
                            "updated": true,
                            "change": change_json(&change, Some(new_id.to_hex())),
                        }))?
                    );
                } else {
                    println!("Updated change {} to {:?}", change.id, change.status);
                }
            }
        }
    }
    Ok(())
}

fn change_json(change: &Change, object_id: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "id": change.id.to_string(),
        "object_id": object_id,
        "intent_id": change.intent_id.to_string(),
        "status": change_status(change.status),
        "head_revision": change.head_revision.map(|id| id.to_hex()),
        "workstream_id": change.workstream_id,
        "created_at_ms": change.created_at_ms,
        "updated_at_ms": change.updated_at_ms,
    })
}

fn change_status(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Open => "open",
        ChangeStatus::Ready => "ready",
        ChangeStatus::Integrated => "integrated",
        ChangeStatus::Abandoned => "abandoned",
    }
}
