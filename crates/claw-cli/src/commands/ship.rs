use clap::Args;

use claw_core::object::Object;
use claw_core::types::{CapsulePublic, IntentStatus};
use claw_crypto::capsule::build_capsule;
use claw_crypto::keypair::KeyPair;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct ShipArgs {
    /// Intent ID to ship
    #[arg(short, long)]
    intent: String,
    /// Revision ref to ship
    #[arg(short, long, default_value = "heads/main")]
    revision_ref: String,
    /// Agent ID
    #[arg(short, long, default_value = "claw-cli")]
    agent: String,
}

pub fn run(args: ShipArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    // Load intent
    let intent_obj_id = store
        .get_ref(&format!("intents/{}", args.intent))?
        .ok_or_else(|| anyhow::anyhow!("intent not found: {}", args.intent))?;
    let intent_obj = store.load_object(&intent_obj_id)?;
    let intent = match intent_obj {
        Object::Intent(i) => i,
        _ => anyhow::bail!("not an intent"),
    };

    // Load revision
    let rev_id = store
        .get_ref(&args.revision_ref)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.revision_ref))?;

    // Generate ephemeral keypair for signing
    let keypair = KeyPair::generate();

    let public = CapsulePublic {
        agent_id: args.agent.clone(),
        agent_version: None,
        toolchain_digest: None,
        env_fingerprint: None,
        evidence: vec![],
    };

    let capsule = build_capsule(&rev_id, public, None, None, &keypair)?;
    let capsule_id = store.store_object(&Object::Capsule(capsule))?;

    // Update intent status to done
    let mut updated_intent = intent;
    updated_intent.status = IntentStatus::Done;
    updated_intent.updated_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let new_intent_id = store.store_object(&Object::Intent(updated_intent.clone()))?;
    store.set_ref(&format!("intents/{}", updated_intent.id), &new_intent_id)?;

    println!("Shipped intent: {}", updated_intent.id);
    println!("  Capsule: {capsule_id}");
    println!("  Revision: {rev_id}");

    Ok(())
}
