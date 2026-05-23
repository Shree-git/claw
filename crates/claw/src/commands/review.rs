use clap::Args;

use claw_core::id::ChangeId;
use claw_core::object::Object;
use claw_core::types::{Capsule, Change, ChangeStatus, Intent, IntentStatus};
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::object_refs::load_default_capsule;

#[derive(Args)]
pub struct ReviewArgs {
    /// Output review as JSON
    #[arg(long)]
    json: bool,
    /// Restrict review to one intent ID
    #[arg(long)]
    intent: Option<String>,
    /// Restrict review to one change ID
    #[arg(long)]
    change: Option<String>,
    /// Restrict review to one capsule ref or object ID
    #[arg(long)]
    capsule: Option<String>,
}

pub fn run(args: ReviewArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let capsule_filter = args
        .capsule
        .as_deref()
        .map(|value| super::object_refs::load_capsule(&store, value).map(|(id, _)| id))
        .transpose()?;
    let filters = ReviewFilters {
        intent: args.intent.as_deref(),
        change: args.change.as_deref(),
        capsule: capsule_filter,
    };
    let report = build_review_report(&store, &filters)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_review_report(&report);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ReviewFilters<'a> {
    intent: Option<&'a str>,
    change: Option<&'a str>,
    capsule: Option<claw_core::id::ObjectId>,
}

fn build_review_report(
    store: &ClawStore,
    filters: &ReviewFilters<'_>,
) -> anyhow::Result<serde_json::Value> {
    let mut intents = Vec::new();
    for (_name, object_id) in store.list_refs("intents")? {
        let Object::Intent(intent) = store.load_object(&object_id)? else {
            continue;
        };
        if filters
            .intent
            .is_some_and(|filter| filter != intent.id.to_string())
        {
            continue;
        }
        let intent_json = intent_review_json(store, &intent, object_id, filters)?;
        if !intent_json["changes"]
            .as_array()
            .is_some_and(|changes| changes.is_empty())
            || (filters.change.is_none() && filters.capsule.is_none())
        {
            intents.push(intent_json);
        }
    }

    let change_index = review_change_index(&intents);
    let capsule_index = review_capsule_index(&intents);
    let summary = review_summary(&intents, &change_index, &capsule_index);

    Ok(serde_json::json!({
        "schema_version": 1,
        "action": "review",
        "intent_count": intents.len(),
        "change_count": change_index.len(),
        "capsule_count": capsule_index.len(),
        "filters": {
            "intent": filters.intent,
            "change": filters.change,
            "capsule": filters.capsule.map(|id| id.to_hex()),
        },
        "summary": summary,
        "index": {
            "changes": change_index,
            "capsules": capsule_index,
        },
        "intents": intents,
    }))
}

fn review_change_index(intents: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for intent in intents {
        let Some(intent_id) = intent["id"].as_str() else {
            continue;
        };
        for change in intent["changes"].as_array().into_iter().flatten() {
            let Some(change_id) = change["id"].as_str() else {
                continue;
            };
            let revision = &change["head_revision"];
            let capsule = &revision["capsule"];
            out.push(serde_json::json!({
                "intent_id": intent_id,
                "change_id": change_id,
                "change_status": change["status"],
                "revision": if revision.is_null() { None } else { revision["hex"].as_str() },
                "capsule": capsule["hex"].as_str(),
                "capsule_present": capsule["present"].as_bool().unwrap_or(false),
                "evidence_count": capsule["evidence_count"].as_u64().unwrap_or(0),
                "signature_count": capsule["signature_count"].as_u64().unwrap_or(0),
            }));
        }
    }
    out
}

fn review_capsule_index(intents: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for intent in intents {
        let Some(intent_id) = intent["id"].as_str() else {
            continue;
        };
        for change in intent["changes"].as_array().into_iter().flatten() {
            let Some(change_id) = change["id"].as_str() else {
                continue;
            };
            let revision = &change["head_revision"];
            let capsule = &revision["capsule"];
            if !capsule["present"].as_bool().unwrap_or(false) {
                continue;
            }
            out.push(serde_json::json!({
                "intent_id": intent_id,
                "change_id": change_id,
                "revision": revision["hex"].as_str(),
                "capsule": capsule["hex"].as_str(),
                "capsule_display": capsule["id"].as_str(),
                "agent_id": capsule["agent_id"].as_str(),
                "evidence_count": capsule["evidence_count"].as_u64().unwrap_or(0),
                "signature_count": capsule["signature_count"].as_u64().unwrap_or(0),
                "has_private_fields": capsule["has_private_fields"].as_bool().unwrap_or(false),
                "recipient_count": capsule["recipient_count"].as_u64().unwrap_or(0),
            }));
        }
    }
    out
}

fn review_summary(
    intents: &[serde_json::Value],
    change_index: &[serde_json::Value],
    capsule_index: &[serde_json::Value],
) -> serde_json::Value {
    let blocked_intent_count = intents
        .iter()
        .filter(|intent| intent["status"].as_str() == Some("blocked"))
        .count();
    let missing_policy_count = intents
        .iter()
        .flat_map(|intent| intent["policies"].as_array().into_iter().flatten())
        .filter(|policy| !policy["present"].as_bool().unwrap_or(false))
        .count();
    let missing_capsule_count = change_index
        .iter()
        .filter(|change| !change["capsule_present"].as_bool().unwrap_or(false))
        .count();
    let unsigned_capsule_count = capsule_index
        .iter()
        .filter(|capsule| capsule["signature_count"].as_u64().unwrap_or(0) == 0)
        .count();
    let private_capsule_count = capsule_index
        .iter()
        .filter(|capsule| capsule["has_private_fields"].as_bool().unwrap_or(false))
        .count();
    let failing_evidence_count = intents
        .iter()
        .flat_map(|intent| intent["changes"].as_array().into_iter().flatten())
        .filter_map(|change| change["head_revision"]["capsule"]["evidence"].as_array())
        .flatten()
        .filter(|evidence| {
            evidence["status"]
                .as_str()
                .is_some_and(|status| !status.eq_ignore_ascii_case("pass"))
        })
        .count();
    let total_evidence_count = capsule_index
        .iter()
        .map(|capsule| capsule["evidence_count"].as_u64().unwrap_or(0))
        .sum::<u64>();
    let review_required = blocked_intent_count > 0
        || missing_policy_count > 0
        || missing_capsule_count > 0
        || unsigned_capsule_count > 0
        || failing_evidence_count > 0;

    serde_json::json!({
        "review_required": review_required,
        "blocked_intent_count": blocked_intent_count,
        "missing_policy_count": missing_policy_count,
        "missing_capsule_count": missing_capsule_count,
        "unsigned_capsule_count": unsigned_capsule_count,
        "private_capsule_count": private_capsule_count,
        "total_evidence_count": total_evidence_count,
        "failing_evidence_count": failing_evidence_count,
    })
}

fn intent_review_json(
    store: &ClawStore,
    intent: &Intent,
    object_id: claw_core::id::ObjectId,
    filters: &ReviewFilters<'_>,
) -> anyhow::Result<serde_json::Value> {
    let mut changes = Vec::new();
    for change_id in &intent.change_ids {
        if filters
            .change
            .is_some_and(|filter| filter != change_id.to_string().as_str())
        {
            continue;
        }
        changes.push(match load_change(store, change_id) {
            Ok((change_object_id, change)) => {
                let change_json = change_review_json(store, change_object_id, &change)?;
                if filters.capsule.is_some_and(|expected| {
                    nested_capsule_hex(&change_json)
                        .and_then(|hex| claw_core::id::ObjectId::from_hex(hex).ok())
                        != Some(expected)
                }) {
                    continue;
                }
                change_json
            }
            Err(err) => serde_json::json!({
                "id": change_id,
                "load_error": err.to_string(),
            }),
        });
    }

    let policy_refs = intent
        .policy_refs
        .iter()
        .map(|policy_ref| {
            let ref_name = if policy_ref.starts_with("policies/") {
                policy_ref.clone()
            } else {
                format!("policies/{policy_ref}")
            };
            match store.get_ref(&ref_name) {
                Ok(Some(id)) => serde_json::json!({
                    "id": policy_ref,
                    "ref": ref_name,
                    "object": id.to_hex(),
                    "present": true,
                }),
                Ok(None) => serde_json::json!({
                    "id": policy_ref,
                    "ref": ref_name,
                    "present": false,
                }),
                Err(err) => serde_json::json!({
                    "id": policy_ref,
                    "ref": ref_name,
                    "present": false,
                    "error": err.to_string(),
                }),
            }
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "id": intent.id.to_string(),
        "object": object_id.to_hex(),
        "title": intent.title,
        "goal": intent.goal,
        "status": intent_status(intent.status),
        "acceptance_tests": intent.acceptance_tests,
        "policies": policy_refs,
        "agents": intent.agents,
        "depends_on": intent.depends_on,
        "supersedes": intent.supersedes,
        "changes": changes,
        "review": {
            "change_count": changes.len(),
            "acceptance_test_count": intent.acceptance_tests.len(),
            "policy_count": intent.policy_refs.len(),
        },
    }))
}

fn change_review_json(
    store: &ClawStore,
    object_id: claw_core::id::ObjectId,
    change: &Change,
) -> anyhow::Result<serde_json::Value> {
    let revision = match change.head_revision {
        Some(revision_id) => revision_review_json(store, revision_id),
        None => Ok(serde_json::json!(null)),
    }?;

    Ok(serde_json::json!({
        "id": change.id.to_string(),
        "object": object_id.to_hex(),
        "intent_id": change.intent_id.to_string(),
        "status": change_status(change.status),
        "head_revision": revision,
        "workstream_id": change.workstream_id,
        "created_at_ms": change.created_at_ms,
        "updated_at_ms": change.updated_at_ms,
    }))
}

fn revision_review_json(
    store: &ClawStore,
    revision_id: claw_core::id::ObjectId,
) -> anyhow::Result<serde_json::Value> {
    let Object::Revision(revision) = store.load_object(&revision_id)? else {
        anyhow::bail!("head revision is not a revision: {}", revision_id.to_hex());
    };

    let capsule = match load_default_capsule(store, &revision_id, &revision) {
        Ok((capsule_id, capsule)) => capsule_review_json(capsule_id, &capsule),
        Err(err) => serde_json::json!({
            "present": false,
            "error": err.to_string(),
        }),
    };

    Ok(serde_json::json!({
        "id": revision_id.to_string(),
        "hex": revision_id.to_hex(),
        "summary": revision.summary,
        "author": revision.author,
        "created_at_ms": revision.created_at_ms,
        "parents": revision.parents.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
        "patches": revision.patches.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
        "capsule": capsule,
    }))
}

fn capsule_review_json(
    capsule_id: claw_core::id::ObjectId,
    capsule: &Capsule,
) -> serde_json::Value {
    serde_json::json!({
        "present": true,
        "id": capsule_id.to_string(),
        "hex": capsule_id.to_hex(),
        "agent_id": capsule.public_fields.agent_id,
        "agent_version": capsule.public_fields.agent_version,
        "toolchain_digest": capsule.public_fields.toolchain_digest,
        "environment_fingerprint": capsule.public_fields.env_fingerprint,
        "evidence_count": capsule.public_fields.evidence.len(),
        "evidence": capsule.public_fields.evidence,
        "signature_count": capsule.signatures.len(),
        "signers": capsule.signatures.iter().map(|signature| &signature.signer_id).collect::<Vec<_>>(),
        "has_private_fields": capsule.encrypted_private.is_some(),
        "recipient_count": capsule.recipients.len(),
    })
}

fn nested_capsule_hex(change: &serde_json::Value) -> Option<&str> {
    change["head_revision"]["capsule"]["hex"].as_str()
}

fn load_change(store: &ClawStore, id: &str) -> anyhow::Result<(claw_core::id::ObjectId, Change)> {
    let change_id = ChangeId::from_string(id)?;
    let ref_name = format!("changes/{change_id}");
    let object_id = store
        .get_ref(&ref_name)?
        .ok_or_else(|| anyhow::anyhow!("change not found: {change_id}"))?;
    match store.load_object(&object_id)? {
        Object::Change(change) => Ok((object_id, change)),
        _ => anyhow::bail!("ref does not point to a change object: {ref_name}"),
    }
}

fn print_review_report(report: &serde_json::Value) {
    let intents = report["intents"].as_array().cloned().unwrap_or_default();
    if intents.is_empty() {
        println!("No intents to review.");
        return;
    }

    for intent in intents {
        println!(
            "Intent {} [{}]: {}",
            intent["id"].as_str().unwrap_or(""),
            intent["status"].as_str().unwrap_or("unknown"),
            intent["title"].as_str().unwrap_or("")
        );
        if let Some(goal) = intent["goal"].as_str().filter(|goal| !goal.is_empty()) {
            println!("  Goal: {goal}");
        }
        println!(
            "  Acceptance tests: {}",
            intent["review"]["acceptance_test_count"]
                .as_u64()
                .unwrap_or(0)
        );
        println!(
            "  Policies: {}",
            intent["review"]["policy_count"].as_u64().unwrap_or(0)
        );
        let changes = intent["changes"].as_array().cloned().unwrap_or_default();
        if changes.is_empty() {
            println!("  Changes: none");
        } else {
            println!("  Changes:");
            for change in changes {
                println!(
                    "    {} [{}]",
                    change["id"].as_str().unwrap_or(""),
                    change["status"].as_str().unwrap_or("unknown")
                );
                let revision = &change["head_revision"];
                if revision.is_null() {
                    println!("      Revision: none");
                    continue;
                }
                println!(
                    "      Revision: {} {}",
                    revision["hex"].as_str().unwrap_or(""),
                    revision["summary"].as_str().unwrap_or("")
                );
                let capsule = &revision["capsule"];
                if capsule["present"].as_bool().unwrap_or(false) {
                    println!(
                        "      Capsule: {} evidence={} signatures={}",
                        capsule["hex"].as_str().unwrap_or(""),
                        capsule["evidence_count"].as_u64().unwrap_or(0),
                        capsule["signature_count"].as_u64().unwrap_or(0)
                    );
                } else {
                    println!(
                        "      Capsule: missing ({})",
                        capsule["error"].as_str().unwrap_or("not found")
                    );
                }
            }
        }
    }
    println!(
        "Review index: {} change(s), {} capsule(s)",
        report["change_count"].as_u64().unwrap_or(0),
        report["capsule_count"].as_u64().unwrap_or(0)
    );
    let summary = &report["summary"];
    println!(
        "Review summary: required={} blocked_intents={} missing_capsules={} unsigned_capsules={} failing_evidence={} missing_policies={}",
        summary["review_required"].as_bool().unwrap_or(false),
        summary["blocked_intent_count"].as_u64().unwrap_or(0),
        summary["missing_capsule_count"].as_u64().unwrap_or(0),
        summary["unsigned_capsule_count"].as_u64().unwrap_or(0),
        summary["failing_evidence_count"].as_u64().unwrap_or(0),
        summary["missing_policy_count"].as_u64().unwrap_or(0)
    );
}

fn intent_status(status: IntentStatus) -> &'static str {
    match status {
        IntentStatus::Open => "open",
        IntentStatus::Blocked => "blocked",
        IntentStatus::Done => "done",
        IntentStatus::Superseded => "superseded",
    }
}

fn change_status(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Open => "open",
        ChangeStatus::Ready => "ready",
        ChangeStatus::Integrated => "integrated",
        ChangeStatus::Abandoned => "abandoned",
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use claw_core::id::IntentId;

    use super::ReviewArgs;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: ReviewArgs,
    }

    #[test]
    fn parses_review_intent_filter() {
        let cli = TestCli::parse_from(["claw", "--json", "--intent", "01H"]);
        assert!(cli.args.json);
        assert_eq!(cli.args.intent.as_deref(), Some("01H"));
        assert!(cli.args.change.is_none());
        assert!(cli.args.capsule.is_none());
    }

    #[test]
    fn parses_review_defaults() {
        let cli = TestCli::parse_from(["claw"]);
        assert!(!cli.args.json);
        assert!(cli.args.intent.is_none());
        assert!(cli.args.change.is_none());
        assert!(cli.args.capsule.is_none());
    }

    #[test]
    fn parses_review_change_and_capsule_filters() {
        let cli = TestCli::parse_from(["claw", "--change", "chg", "--capsule", "clw_abc"]);
        assert_eq!(cli.args.change.as_deref(), Some("chg"));
        assert_eq!(cli.args.capsule.as_deref(), Some("clw_abc"));
    }

    #[test]
    fn intent_id_parser_is_available_for_filters() {
        assert!(IntentId::from_string("01H00000000000000000000000").is_ok());
    }
}
