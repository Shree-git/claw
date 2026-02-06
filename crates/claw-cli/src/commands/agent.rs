use clap::{Args, Subcommand};

use claw_core::object::Object;
use claw_core::types::CapsulePublic;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Register a new agent
    Register {
        /// Agent ID
        #[arg(short, long)]
        name: String,
        /// Agent version
        #[arg(short, long)]
        version: Option<String>,
    },
    /// Show agent status
    Status {
        /// Agent name
        name: Option<String>,
    },
    /// List registered agents
    List,
}

pub fn run(args: AgentArgs) -> anyhow::Result<()> {
    match args.command {
        AgentCommand::Register { name, version } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;

            // Store agent config as a capsule public record
            let agent_record = CapsulePublic {
                agent_id: name.clone(),
                agent_version: version.clone(),
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![],
            };

            let serialized = serde_json::to_vec(&agent_record)
                .map_err(|e| anyhow::anyhow!("serialization failed: {e}"))?;
            let blob = Object::Blob(claw_core::types::Blob {
                data: serialized,
                media_type: Some("application/json".to_string()),
            });
            let id = store.store_object(&blob)?;
            store.set_ref(&format!("agents/{name}"), &id)?;

            println!("Registered agent: {name}");
            if let Some(v) = version {
                println!("  Version: {v}");
            }
            println!("  Object: {id}");
        }
        AgentCommand::Status { name } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;

            if let Some(n) = name {
                if let Ok(Some(id)) = store.get_ref(&format!("agents/{n}")) {
                    if let Ok(Object::Blob(b)) = store.load_object(&id) {
                        if let Ok(agent) = serde_json::from_slice::<CapsulePublic>(&b.data) {
                            println!("Agent: {}", agent.agent_id);
                            if let Some(v) = &agent.agent_version {
                                println!("  Version: {v}");
                            }
                            println!("  Status: active");
                        }
                    }
                } else {
                    println!("Agent {n}: not found");
                }
            } else {
                println!("Use 'claw agent list' to see all agents.");
            }
        }
        AgentCommand::List => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("agents")?;
            if refs.is_empty() {
                println!("No agents registered.");
            } else {
                for (name, id) in &refs {
                    if let Ok(Object::Blob(b)) = store.load_object(id) {
                        if let Ok(agent) = serde_json::from_slice::<CapsulePublic>(&b.data) {
                            println!("{} v{}", agent.agent_id, agent.agent_version.as_deref().unwrap_or("?"));
                        } else {
                            println!("{name}");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
