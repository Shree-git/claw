use std::collections::BTreeSet;

use clap::{Args, Subcommand};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Capsule, CapsuleSignature};
use claw_crypto::capsule::verify_capsule;
use claw_store::ClawStore;
use serde_json::Value;

use crate::config::find_repo_root;

use super::agent::AgentRegistration;
use super::object_refs::{derive_capsule_trust_score, load_capsule};

#[derive(Args)]
pub struct CapsuleArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: CapsuleCommand,
}

#[derive(Subcommand)]
enum CapsuleCommand {
    /// List stored capsules
    List,
    /// Inspect a capsule or a revision with an attached capsule
    Inspect {
        /// Capsule ref, capsule object id, revision ref, or revision object id
        target: String,
    },
}

pub fn run(args: CapsuleArgs) -> anyhow::Result<()> {
    match args.command {
        CapsuleCommand::List => run_list(args.json),
        CapsuleCommand::Inspect { target } => run_inspect(&target, args.json),
    }
}

fn run_list(json: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let mut rows = Vec::new();

    for id in store.list_object_ids()? {
        if let Ok(Object::Capsule(capsule)) = store.load_object(&id) {
            rows.push(capsule_report(&store, &id, &capsule));
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "capsule.list",
                "count": rows.len(),
                "capsules": rows,
            }))?
        );
    } else if rows.is_empty() {
        println!("No capsules found.");
    } else {
        for row in rows {
            println!(
                "{} revision={} agent={} evidence={} signatures={} private={}",
                row["id"].as_str().unwrap_or_default(),
                row["revision_hex"].as_str().unwrap_or_default(),
                row["agent_id"].as_str().unwrap_or_default(),
                row["evidence_count"].as_u64().unwrap_or_default(),
                row["signature_count"].as_u64().unwrap_or_default(),
                row["has_private_fields"].as_bool().unwrap_or(false)
            );
        }
    }

    Ok(())
}

fn run_inspect(target: &str, json: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let (capsule_id, capsule) = load_capsule(&store, target)?;
    let mut body = capsule_report(&store, &capsule_id, &capsule);

    if json {
        body["schema_version"] = serde_json::json!(1);
        body["action"] = serde_json::json!("capsule.inspect");
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("Capsule: {}", capsule_id.to_hex());
        println!("  Revision: {}", capsule.revision_id.to_hex());
        println!("  Agent: {}", capsule.public_fields.agent_id);
        if let Some(version) = &capsule.public_fields.agent_version {
            println!("  Agent version: {version}");
        }
        if let Some(digest) = &capsule.public_fields.toolchain_digest {
            println!("  Toolchain digest: {digest}");
        }
        if let Some(env) = &capsule.public_fields.env_fingerprint {
            println!("  Environment: {env}");
        }
        println!(
            "  Trust score: {}",
            derive_capsule_trust_score(&capsule)
                .map(|score| format!("{score:.2}"))
                .unwrap_or_else(|| "n/a".to_string())
        );
        println!("  Evidence:");
        if capsule.public_fields.evidence.is_empty() {
            println!("    none");
        } else {
            for evidence in &capsule.public_fields.evidence {
                println!(
                    "    {}={} duration_ms={}",
                    evidence.name, evidence.status, evidence.duration_ms
                );
                if let Some(command) = &evidence.command {
                    println!("      command: {command}");
                }
                if let Some(runner) = &evidence.runner_identity {
                    println!("      runner: {runner}");
                }
            }
        }
        println!("  Signatures:");
        if capsule.signatures.is_empty() {
            println!("    none");
        } else {
            for signature in &capsule.signatures {
                println!(
                    "    signer={} bytes={}",
                    signature.signer_id,
                    signature.signature.len()
                );
            }
        }
        println!(
            "  Private fields: {}",
            if capsule.encrypted_private.is_some() {
                "encrypted"
            } else {
                "none"
            }
        );
        if !capsule.recipients.is_empty() {
            println!("  Recipients:");
            for recipient in &capsule.recipients {
                println!(
                    "    {} key={} algorithm={}",
                    recipient.recipient_id, recipient.key_id, recipient.algorithm
                );
            }
        }
        println!("  Trust path:");
        let trust_path = &body["trust_path"];
        println!(
            "    registered_agent={} status={} signature_verified={} registered_signature_verified={}",
            trust_path["agent"]["registered"].as_bool().unwrap_or(false),
            trust_path["agent"]["status"].as_str().unwrap_or("missing"),
            trust_path["cryptographic_integrity"].as_bool().unwrap_or(false),
            trust_path["registered_identity_verified"]
                .as_bool()
                .unwrap_or(false)
        );
        for reason in trust_path["reasons"].as_array().into_iter().flatten() {
            if let Some(reason) = reason.as_str() {
                println!("    - {reason}");
            }
        }
    }

    Ok(())
}

pub(crate) fn capsule_report(store: &ClawStore, id: &ObjectId, capsule: &Capsule) -> Value {
    let agents = load_agent_records(store);
    let signatures = signature_reports(capsule, &agents);
    let private_fields = private_fields_report(capsule);
    let trust_path = trust_path_report(id, capsule, &agents, &signatures, &private_fields);
    let agent_identity = agent_identity_report(capsule, &signatures, &trust_path);
    let execution_environment = execution_environment_report(capsule);

    serde_json::json!({
        "id": id.to_string(),
        "hex": id.to_hex(),
        "revision_id": capsule.revision_id.to_string(),
        "revision_hex": capsule.revision_id.to_hex(),
        "agent_id": capsule.public_fields.agent_id,
        "agent_version": capsule.public_fields.agent_version,
        "toolchain_digest": capsule.public_fields.toolchain_digest,
        "environment_fingerprint": capsule.public_fields.env_fingerprint,
        "trust_score": derive_capsule_trust_score(capsule),
        "evidence_count": capsule.public_fields.evidence.len(),
        "evidence": capsule.public_fields.evidence,
        "signature_count": capsule.signatures.len(),
        "signatures": signatures,
        "agent_identity": agent_identity,
        "execution_environment": execution_environment,
        "has_private_fields": capsule.encrypted_private.is_some(),
        "private_bytes": capsule.encrypted_private.as_ref().map(Vec::len),
        "encryption": capsule.encryption,
        "key_id": capsule.key_id,
        "recipients": private_fields["recipients"].clone(),
        "private_fields": private_fields,
        "trust_path": trust_path,
    })
}

#[derive(Debug, Clone)]
struct AgentRecordSummary {
    ref_name: String,
    object_hex: String,
    agent_id: String,
    agent_version: Option<String>,
    public_key: String,
    status: &'static str,
    revoked_at_ms: Option<u64>,
    quarantined_at_ms: Option<u64>,
}

fn load_agent_records(store: &ClawStore) -> Vec<AgentRecordSummary> {
    let Ok(refs) = store.list_refs("agents") else {
        return Vec::new();
    };

    refs.into_iter()
        .filter_map(|(ref_name, object_id)| {
            let Ok(Object::Blob(blob)) = store.load_object(&object_id) else {
                return None;
            };
            let Ok(record) = serde_json::from_slice::<AgentRegistration>(&blob.data) else {
                return None;
            };
            let status = record.lifecycle_status();
            Some(AgentRecordSummary {
                ref_name,
                object_hex: object_id.to_hex(),
                agent_id: record.agent_id,
                agent_version: record.agent_version,
                public_key: record.public_key.to_ascii_lowercase(),
                status,
                revoked_at_ms: record.revoked_at_ms,
                quarantined_at_ms: record.quarantined_at_ms,
            })
        })
        .collect()
}

fn agent_identity_report(capsule: &Capsule, signatures: &[Value], trust_path: &Value) -> Value {
    let signer_ids = signatures
        .iter()
        .filter_map(|signature| signature["signer_id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let verified_signature_count = signatures
        .iter()
        .filter(|signature| signature["verified"].as_bool().unwrap_or(false))
        .count();
    let registered_agent = trust_path["agent"]["registration"].clone();

    serde_json::json!({
        "claimed_agent_id": capsule.public_fields.agent_id,
        "claimed_agent_version": capsule.public_fields.agent_version,
        "registered": trust_path["agent"]["registered"].as_bool().unwrap_or(false),
        "status": trust_path["agent"]["status"].as_str().unwrap_or("missing"),
        "registered_agent": registered_agent,
        "signer_ids": signer_ids,
        "signature_count": signatures.len(),
        "verified_signature_count": verified_signature_count,
        "registered_identity_verified": trust_path["registered_identity_verified"].as_bool().unwrap_or(false),
    })
}

fn execution_environment_report(capsule: &Capsule) -> Value {
    let runner_identities = unique_optional_strings(
        capsule
            .public_fields
            .evidence
            .iter()
            .filter_map(|item| item.runner_identity.as_deref()),
    );
    let commands = unique_optional_strings(
        capsule
            .public_fields
            .evidence
            .iter()
            .filter_map(|item| item.command.as_deref()),
    );
    let environment_digests = unique_optional_strings(
        capsule
            .public_fields
            .evidence
            .iter()
            .filter_map(|item| item.environment_digest.as_deref()),
    );
    let log_digests = unique_optional_strings(
        capsule
            .public_fields
            .evidence
            .iter()
            .filter_map(|item| item.log_digest.as_deref()),
    );
    let artifact_digests = unique_optional_strings(
        capsule
            .public_fields
            .evidence
            .iter()
            .filter_map(|item| item.artifact_digest.as_deref()),
    );

    serde_json::json!({
        "toolchain_digest": capsule.public_fields.toolchain_digest,
        "environment_fingerprint": capsule.public_fields.env_fingerprint,
        "runner_identities": runner_identities,
        "commands": commands,
        "environment_digests": environment_digests,
        "log_digests": log_digests,
        "artifact_digests": artifact_digests,
        "evidence_count": capsule.public_fields.evidence.len(),
        "evidence_with_runner_count": capsule.public_fields.evidence.iter().filter(|item| item.runner_identity.is_some()).count(),
        "evidence_with_environment_digest_count": capsule.public_fields.evidence.iter().filter(|item| item.environment_digest.is_some()).count(),
        "evidence_with_log_digest_count": capsule.public_fields.evidence.iter().filter(|item| item.log_digest.is_some()).count(),
        "evidence_with_artifact_digest_count": capsule.public_fields.evidence.iter().filter(|item| item.artifact_digest.is_some()).count(),
    })
}

fn unique_optional_strings<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn signature_reports(capsule: &Capsule, agents: &[AgentRecordSummary]) -> Vec<Value> {
    capsule
        .signatures
        .iter()
        .map(|signature| {
            let signer_key = signature.signer_id.to_ascii_lowercase();
            let registered = agents.iter().find(|agent| agent.public_key == signer_key);
            let verification = verify_signature(capsule, signature);
            serde_json::json!({
                "signer_id": signature.signer_id,
                "signer_id_prefix": prefix(&signature.signer_id, 16),
                "signature_bytes": signature.signature.len(),
                "signature_prefix": prefix(&hex::encode(&signature.signature), 32),
                "verified": verification.unwrap_or(false),
                "verification_error": if verification.is_none() { Some("signer_id is not a 32-byte Ed25519 public key or signature is malformed") } else { None },
                "registered_agent": registered.map(agent_record_json),
            })
        })
        .collect()
}

fn verify_signature(capsule: &Capsule, signature: &CapsuleSignature) -> Option<bool> {
    let public_key = decode_32_hex(&signature.signer_id)?;
    let mut single_signature_capsule = capsule.clone();
    single_signature_capsule.signatures = vec![signature.clone()];
    verify_capsule(&single_signature_capsule, &public_key).ok()
}

fn private_fields_report(capsule: &Capsule) -> Value {
    let recipients = capsule
        .recipients
        .iter()
        .map(|recipient| {
            serde_json::json!({
                "recipient_id": recipient.recipient_id,
                "key_id": recipient.key_id,
                "algorithm": recipient.algorithm,
                "ephemeral_public_key_bytes": recipient.ephemeral_public_key.len(),
                "encrypted_content_key_bytes": recipient.encrypted_content_key.len(),
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "present": capsule.encrypted_private.is_some(),
        "redacted": capsule.encrypted_private.is_some(),
        "ciphertext_bytes": capsule.encrypted_private.as_ref().map(Vec::len),
        "encryption": if capsule.encryption.is_empty() { None } else { Some(capsule.encryption.clone()) },
        "key_id": capsule.key_id,
        "recipient_count": capsule.recipients.len(),
        "recipients": recipients,
    })
}

fn trust_path_report(
    id: &ObjectId,
    capsule: &Capsule,
    agents: &[AgentRecordSummary],
    signatures: &[Value],
    private_fields: &Value,
) -> Value {
    let claimed_agent = agents
        .iter()
        .find(|agent| agent.agent_id == capsule.public_fields.agent_id);
    let claimed_agent_signature_verified = claimed_agent.is_some_and(|agent| {
        signatures.iter().any(|signature| {
            signature["verified"].as_bool().unwrap_or(false)
                && signature["signer_id"]
                    .as_str()
                    .is_some_and(|signer| signer.eq_ignore_ascii_case(&agent.public_key))
        })
    });
    let cryptographic_integrity = signatures
        .iter()
        .any(|signature| signature["verified"].as_bool().unwrap_or(false));

    let mut reasons = Vec::new();
    if claimed_agent.is_none() {
        reasons.push(format!(
            "claimed agent '{}' is not registered under agents/*",
            capsule.public_fields.agent_id
        ));
    }
    if let Some(agent) = claimed_agent {
        if agent.status != "active" {
            reasons.push(format!(
                "claimed agent '{}' is {}",
                capsule.public_fields.agent_id, agent.status
            ));
        }
    }
    if capsule.signatures.is_empty() {
        reasons.push("capsule has no signatures".to_string());
    } else if !cryptographic_integrity {
        reasons.push("no capsule signature verifies against its signer_id".to_string());
    }
    if !claimed_agent_signature_verified {
        reasons.push("no verified signature matches the claimed agent registration".to_string());
    }
    if private_fields["present"].as_bool().unwrap_or(false)
        && private_fields["recipient_count"].as_u64().unwrap_or(0) == 0
    {
        reasons.push("private fields are present without recipient envelopes".to_string());
    }
    if reasons.is_empty() {
        reasons.push("registered active agent signature verifies capsule claim".to_string());
    }

    serde_json::json!({
        "capsule": {
            "id": id.to_string(),
            "hex": id.to_hex(),
        },
        "revision": {
            "id": capsule.revision_id.to_string(),
            "hex": capsule.revision_id.to_hex(),
        },
        "agent": {
            "claimed_id": capsule.public_fields.agent_id,
            "claimed_version": capsule.public_fields.agent_version,
            "registered": claimed_agent.is_some(),
            "status": claimed_agent.map(|agent| agent.status).unwrap_or("missing"),
            "registration": claimed_agent.map(agent_record_json),
        },
        "toolchain_digest": capsule.public_fields.toolchain_digest,
        "environment_fingerprint": capsule.public_fields.env_fingerprint,
        "evidence": {
            "total": capsule.public_fields.evidence.len(),
            "passing": capsule.public_fields.evidence.iter().filter(|item| item.status.eq_ignore_ascii_case("pass")).count(),
            "failing": capsule.public_fields.evidence.iter().filter(|item| !item.status.eq_ignore_ascii_case("pass")).count(),
            "items": capsule.public_fields.evidence.iter().map(|item| serde_json::json!({
                "name": item.name,
                "status": item.status,
                "runner_identity": item.runner_identity,
                "environment_digest": item.environment_digest,
                "log_digest": item.log_digest,
                "artifact_digest": item.artifact_digest,
                "expires_at_ms": item.expires_at_ms,
            })).collect::<Vec<_>>(),
        },
        "signatures": signatures,
        "private_fields": private_fields,
        "cryptographic_integrity": cryptographic_integrity,
        "registered_identity_verified": claimed_agent_signature_verified,
        "reasons": reasons,
    })
}

fn agent_record_json(agent: &AgentRecordSummary) -> Value {
    serde_json::json!({
        "ref": agent.ref_name,
        "object": agent.object_hex,
        "agent_id": agent.agent_id,
        "agent_version": agent.agent_version,
        "public_key": agent.public_key,
        "public_key_prefix": prefix(&agent.public_key, 16),
        "status": agent.status,
        "revoked_at_ms": agent.revoked_at_ms,
        "quarantined_at_ms": agent.quarantined_at_ms,
    })
}

fn decode_32_hex(value: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(value).ok()?;
    bytes.as_slice().try_into().ok()
}

fn prefix(value: &str, chars: usize) -> String {
    value.chars().take(chars).collect()
}
