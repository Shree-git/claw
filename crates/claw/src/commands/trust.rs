use clap::{Args, Subcommand};

use claw_policy::{evaluator::evaluate_policy, PolicyContext};
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::capsule::capsule_report;
use super::object_refs::{
    current_time_ms, derive_capsule_trust_score, load_capsule, load_default_capsule, load_policy,
    load_revision,
};

#[derive(Args)]
pub struct TrustArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: TrustCommand,
}

#[derive(Subcommand)]
enum TrustCommand {
    /// Print why a revision is or is not trustworthy
    Receipt {
        /// Revision ref, hex ID, or clw_ display ID
        #[arg(long)]
        revision: String,
        /// Capsule ref or object ID. Defaults to the revision capsule.
        #[arg(long)]
        capsule: Option<String>,
        /// Policy ID/ref to evaluate. Repeatable. Defaults to all stored policies.
        #[arg(long = "policy")]
        policies: Vec<String>,
        /// Verified signer agent ID (repeatable)
        #[arg(long = "signer-agent")]
        signer_agents: Vec<String>,
        /// Verified signer key ID (repeatable)
        #[arg(long = "signer-key")]
        signer_keys: Vec<String>,
        /// Touched path for sensitive-path evaluation (repeatable)
        #[arg(long = "path")]
        touched_paths: Vec<String>,
        /// Trust score override. Defaults to capsule evidence pass ratio.
        #[arg(long)]
        trust_score: Option<String>,
    },
}

pub fn run(args: TrustArgs) -> anyhow::Result<()> {
    match args.command {
        TrustCommand::Receipt {
            revision,
            capsule,
            policies,
            signer_agents,
            signer_keys,
            touched_paths,
            trust_score,
        } => run_receipt(ReceiptRequest {
            revision,
            capsule,
            policies,
            signer_agents,
            signer_keys,
            touched_paths,
            trust_score,
            json: args.json,
        }),
    }
}

struct ReceiptRequest {
    revision: String,
    capsule: Option<String>,
    policies: Vec<String>,
    signer_agents: Vec<String>,
    signer_keys: Vec<String>,
    touched_paths: Vec<String>,
    trust_score: Option<String>,
    json: bool,
}

fn run_receipt(request: ReceiptRequest) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let (revision_id, revision) = load_revision(&store, &request.revision)?;
    let (capsule_id, capsule) = match request.capsule.as_deref() {
        Some(value) => load_capsule(&store, value)?,
        None => load_default_capsule(&store, &revision_id, &revision)?,
    };
    let policy_ids = if request.policies.is_empty() {
        store
            .list_refs("policies")?
            .into_iter()
            .filter_map(|(name, _)| name.strip_prefix("policies/").map(str::to_string))
            .collect::<Vec<_>>()
    } else {
        request.policies
    };

    let trust_score = request
        .trust_score
        .as_deref()
        .map(parse_trust_score)
        .transpose()?
        .or_else(|| derive_capsule_trust_score(&capsule));
    let context = PolicyContext {
        revision_id: Some(revision_id),
        signer_agent_ids: request.signer_agents,
        signer_key_ids: request.signer_keys,
        touched_paths: request.touched_paths,
        trust_score,
        now_ms: Some(current_time_ms()),
    };

    let mut policy_results = Vec::new();
    for id in &policy_ids {
        let (policy_ref, policy_object, policy) = load_policy(&store, id)?;
        let evaluation = evaluate_policy(&policy, &revision, &capsule, &context);
        policy_results.push(serde_json::json!({
            "id": policy.policy_id,
            "ref": policy_ref,
            "object": policy_object.to_hex(),
            "allowed": evaluation.is_ok(),
            "reason": evaluation.err().map(|err| err.to_string()),
            "required_checks": policy.required_checks,
            "required_reviewers": policy.required_reviewers,
            "min_trust_score": policy.min_trust_score,
        }));
    }
    let policies_allowed = policy_results
        .iter()
        .all(|result| result["allowed"].as_bool().unwrap_or(false));
    let mut capsule_receipt = capsule_report(&store, &capsule_id, &capsule);
    capsule_receipt["trust_score"] = serde_json::json!(trust_score);
    let signatures = capsule_receipt["signatures"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let signature_count = signatures.len();
    let verified_signature_count = signatures
        .iter()
        .filter(|signature| signature["verified"].as_bool().unwrap_or(false))
        .count();
    let trust_path = capsule_receipt["trust_path"].clone();
    let cryptographic_integrity = trust_path["cryptographic_integrity"]
        .as_bool()
        .unwrap_or(false);
    let registered_identity_verified = trust_path["registered_identity_verified"]
        .as_bool()
        .unwrap_or(false);
    let capsule_trustworthy = cryptographic_integrity && registered_identity_verified;
    let trustworthy = policies_allowed && capsule_trustworthy;
    let passed_policy_count = policy_results
        .iter()
        .filter(|result| result["allowed"].as_bool().unwrap_or(false))
        .count();
    let failed_policy_count = policy_results.len().saturating_sub(passed_policy_count);
    let capsule_evidence = capsule
        .public_fields
        .evidence
        .iter()
        .map(|evidence| {
            serde_json::json!({
                "name": evidence.name,
                "status": evidence.status,
                "summary": evidence.summary,
                "command": evidence.command,
                "runner_identity": evidence.runner_identity,
                "log_digest": evidence.log_digest,
                "artifact_digest": evidence.artifact_digest,
                "trust_domain": evidence.trust_domain,
            })
        })
        .collect::<Vec<_>>();
    let passing_capsule_evidence = capsule
        .public_fields
        .evidence
        .iter()
        .filter(|evidence| evidence.status.eq_ignore_ascii_case("pass"))
        .count();
    let failing_capsule_evidence = capsule
        .public_fields
        .evidence
        .iter()
        .filter(|evidence| !evidence.status.eq_ignore_ascii_case("pass"))
        .count();
    let private_recipient_count = capsule_receipt["private_fields"]["recipient_count"]
        .as_u64()
        .unwrap_or(0);
    let why = trust_receipt_reasons(
        trustworthy,
        policies_allowed,
        capsule_trustworthy,
        &policy_results,
        &trust_path,
        passing_capsule_evidence,
        failing_capsule_evidence,
        signature_count,
        verified_signature_count,
        private_recipient_count,
    );
    let receipt = serde_json::json!({
        "schema_version": 1,
        "action": "trust.receipt",
        "trustworthy": trustworthy,
        "summary": {
            "verdict": if trustworthy { "trustworthy" } else { "not_trustworthy" },
            "policies_allowed": policies_allowed,
            "policy_count": policy_results.len(),
            "passed_policy_count": passed_policy_count,
            "failed_policy_count": failed_policy_count,
            "capsule_trustworthy": capsule_trustworthy,
            "cryptographic_integrity": cryptographic_integrity,
            "registered_identity_verified": registered_identity_verified,
            "signature_count": signature_count,
            "verified_signature_count": verified_signature_count,
            "trust_score": trust_score,
            "passing_capsule_evidence": passing_capsule_evidence,
            "failing_capsule_evidence": failing_capsule_evidence,
            "private_recipient_count": private_recipient_count,
        },
        "why": why,
        "revision": {
            "id": revision_id.to_string(),
            "hex": revision_id.to_hex(),
            "author": revision.author,
            "created_at_ms": revision.created_at_ms,
            "summary": revision.summary,
            "change_id": revision.change_id.map(|id| id.to_string()),
            "parents": revision.parents.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
        },
        "capsule": capsule_receipt,
        "provenance": {
            "policy_evidence": revision.policy_evidence.clone(),
            "policy_evidence_count": revision.policy_evidence.len(),
            "capsule_evidence": capsule_evidence,
            "capsule_evidence_count": capsule.public_fields.evidence.len(),
            "passing_capsule_evidence": passing_capsule_evidence,
            "failing_capsule_evidence": failing_capsule_evidence,
            "replay_evidence_count": capsule.public_fields.evidence.iter().filter(|evidence| evidence.name.starts_with("replay:")).count(),
            "attestation_evidence_count": capsule.public_fields.evidence.iter().filter(|evidence| matches!(evidence.name.as_str(), "slsa.provenance" | "in-toto.statement")).count(),
        },
        "context": {
            "signer_agent_ids": context.signer_agent_ids,
            "signer_key_ids": context.signer_key_ids,
            "touched_paths": context.touched_paths,
            "now_ms": context.now_ms,
        },
        "policies": policy_results,
    });

    if request.json {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
    } else {
        let state = if trustworthy {
            "trustworthy"
        } else {
            "not trustworthy"
        };
        println!("Revision {} is {state}.", revision_id.to_hex());
        println!("  Capsule: {}", capsule_id.to_hex());
        println!(
            "  Agent: {}",
            receipt["capsule"]["agent_id"].as_str().unwrap_or("")
        );
        println!(
            "  Trust score: {}",
            trust_score
                .map(|score| format!("{score:.2}"))
                .unwrap_or_else(|| "n/a".to_string())
        );
        println!(
            "  Evidence: {} capsule item(s), {} revision policy evidence item(s), {} signature(s)",
            receipt["provenance"]["capsule_evidence_count"]
                .as_u64()
                .unwrap_or(0),
            receipt["provenance"]["policy_evidence_count"]
                .as_u64()
                .unwrap_or(0),
            receipt["capsule"]["signatures"]
                .as_array()
                .map_or(0, Vec::len)
        );
        println!("  Why:");
        for reason in receipt["why"].as_array().into_iter().flatten() {
            if let Some(reason) = reason.as_str() {
                println!("    - {reason}");
            }
        }
        if receipt["policies"].as_array().is_some_and(Vec::is_empty) {
            println!("  Policies: none configured");
        } else {
            println!("  Policies:");
            for result in receipt["policies"].as_array().into_iter().flatten() {
                let verdict = if result["allowed"].as_bool().unwrap_or(false) {
                    "pass"
                } else {
                    "fail"
                };
                let reason = result["reason"].as_str().unwrap_or("ok");
                println!(
                    "    {}: {verdict} ({reason})",
                    result["id"].as_str().unwrap_or("")
                );
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn trust_receipt_reasons(
    trustworthy: bool,
    policies_allowed: bool,
    capsule_trustworthy: bool,
    policy_results: &[serde_json::Value],
    trust_path: &serde_json::Value,
    passing_capsule_evidence: usize,
    failing_capsule_evidence: usize,
    signature_count: usize,
    verified_signature_count: usize,
    private_recipient_count: u64,
) -> Vec<String> {
    let mut reasons = Vec::new();

    if trustworthy {
        if policy_results.is_empty() {
            reasons.push("no stored policies were configured for this receipt".to_string());
        } else {
            reasons.push(format!(
                "all {} evaluated policy/policies passed",
                policy_results.len()
            ));
        }
        reasons.push("capsule signature verifies the registered active agent identity".to_string());
    } else {
        if !policies_allowed {
            for policy in policy_results
                .iter()
                .filter(|policy| !policy["allowed"].as_bool().unwrap_or(false))
            {
                let id = policy["id"].as_str().unwrap_or("unknown");
                let reason = policy["reason"]
                    .as_str()
                    .unwrap_or("policy denied revision");
                reasons.push(format!("policy '{id}' failed: {reason}"));
            }
        }
        if !capsule_trustworthy {
            for reason in trust_path["reasons"].as_array().into_iter().flatten() {
                if let Some(reason) = reason.as_str() {
                    reasons.push(reason.to_string());
                }
            }
        }
    }

    if signature_count == 0 {
        reasons.push("capsule has no signatures".to_string());
    } else {
        reasons.push(format!(
            "{verified_signature_count}/{signature_count} capsule signature(s) verified"
        ));
    }

    reasons.push(format!(
        "{passing_capsule_evidence} passing and {failing_capsule_evidence} failing capsule evidence item(s)"
    ));

    if private_recipient_count > 0 {
        reasons.push(format!(
            "private capsule fields are encrypted for {private_recipient_count} recipient(s)"
        ));
    }

    reasons
}

fn parse_trust_score(value: &str) -> anyhow::Result<f32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("trust score cannot be empty");
    }

    let parsed = if let Some(percent) = trimmed.strip_suffix('%') {
        percent
            .trim()
            .parse::<f32>()
            .map_err(|_| anyhow::anyhow!("invalid percentage trust score '{}'", value))?
            / 100.0
    } else {
        trimmed
            .parse::<f32>()
            .map_err(|_| anyhow::anyhow!("invalid trust score '{}'", value))?
    };

    if !(0.0..=1.0).contains(&parsed) {
        anyhow::bail!("trust score '{}' must be between 0 and 1", value);
    }

    Ok(parsed)
}
