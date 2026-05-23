use clap::{Args, Subcommand};

use claw_policy::{evaluator::evaluate_policy, PolicyContext};
use claw_store::reflog;
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::object_refs::{
    current_time_ms, derive_capsule_trust_score, load_default_capsule, load_policy, load_revision,
};

#[derive(Args)]
pub struct TimelineArgs {
    /// Output timeline as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: TimelineCommand,
}

#[derive(Subcommand)]
enum TimelineCommand {
    /// Show reflog entries for a ref
    Ref {
        /// Ref name to inspect
        #[arg(long, default_value = "heads/main")]
        ref_name: String,
    },
    /// Show revision parents, capsule, and evidence timeline details
    Revision {
        /// Revision ref, hex ID, or clw_ display ID
        revision: String,
    },
    /// Explain which current policies allow or deny a revision
    Allowed {
        /// Revision ref, hex ID, or clw_ display ID
        #[arg(long)]
        revision: String,
        /// Policy ID/ref. Repeatable. Defaults to all policies.
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
    },
}

pub fn run(args: TimelineArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    match args.command {
        TimelineCommand::Ref { ref_name } => run_ref(&store, &ref_name, args.json),
        TimelineCommand::Revision { revision } => run_revision(&store, &revision, args.json),
        TimelineCommand::Allowed {
            revision,
            policies,
            signer_agents,
            signer_keys,
            touched_paths,
        } => run_allowed(
            &store,
            &revision,
            policies,
            signer_agents,
            signer_keys,
            touched_paths,
            args.json,
        ),
    }
}

fn run_ref(store: &ClawStore, ref_name: &str, json: bool) -> anyhow::Result<()> {
    let entries = reflog::read_reflog(store.layout(), ref_name)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "timeline.ref",
                "ref": ref_name,
                "entries": entries.iter().map(|entry| serde_json::json!({
                    "old": entry.old.to_hex(),
                    "new": entry.new.to_hex(),
                    "timestamp_ms": entry.timestamp_ms,
                    "author": entry.author,
                    "message": entry.message,
                })).collect::<Vec<_>>(),
            }))?
        );
    } else if entries.is_empty() {
        println!("No reflog entries for {ref_name}");
    } else {
        println!("Timeline for {ref_name}");
        for entry in entries {
            println!(
                "  {} {} -> {} {} {}",
                entry.timestamp_ms,
                entry.old.to_hex(),
                entry.new.to_hex(),
                entry.author,
                entry.message
            );
        }
    }
    Ok(())
}

fn run_revision(store: &ClawStore, revision: &str, json: bool) -> anyhow::Result<()> {
    let (revision_id, revision) = load_revision(store, revision)?;
    let capsule = match load_default_capsule(store, &revision_id, &revision) {
        Ok((capsule_id, capsule)) => serde_json::json!({
            "present": true,
            "id": capsule_id.to_hex(),
            "agent_id": capsule.public_fields.agent_id,
            "evidence": capsule.public_fields.evidence,
            "signature_count": capsule.signatures.len(),
            "trust_score": derive_capsule_trust_score(&capsule),
        }),
        Err(err) => serde_json::json!({
            "present": false,
            "error": err.to_string(),
        }),
    };

    let value = serde_json::json!({
        "schema_version": 1,
        "action": "timeline.revision",
        "revision": {
            "id": revision_id.to_string(),
            "hex": revision_id.to_hex(),
            "summary": revision.summary,
            "author": revision.author,
            "created_at_ms": revision.created_at_ms,
            "change_id": revision.change_id.map(|id| id.to_string()),
            "parents": revision.parents.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
            "patches": revision.patches.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
            "capsule": capsule,
        }
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        let revision = &value["revision"];
        println!(
            "Revision {} {}",
            revision["hex"].as_str().unwrap_or(""),
            revision["summary"].as_str().unwrap_or("")
        );
        println!("  Author: {}", revision["author"].as_str().unwrap_or(""));
        println!(
            "  Created: {}",
            revision["created_at_ms"].as_u64().unwrap_or(0)
        );
        println!(
            "  Parents: {}",
            revision["parents"].as_array().map_or(0, Vec::len)
        );
        let capsule = &revision["capsule"];
        if capsule["present"].as_bool().unwrap_or(false) {
            println!(
                "  Capsule: {} agent={} evidence={} signatures={}",
                capsule["id"].as_str().unwrap_or(""),
                capsule["agent_id"].as_str().unwrap_or(""),
                capsule["evidence"].as_array().map_or(0, Vec::len),
                capsule["signature_count"].as_u64().unwrap_or(0)
            );
        } else {
            println!(
                "  Capsule: missing ({})",
                capsule["error"].as_str().unwrap_or("not found")
            );
        }
    }
    Ok(())
}

fn run_allowed(
    store: &ClawStore,
    revision_ref: &str,
    policies: Vec<String>,
    signer_agents: Vec<String>,
    signer_keys: Vec<String>,
    touched_paths: Vec<String>,
    json: bool,
) -> anyhow::Result<()> {
    let (revision_id, revision) = load_revision(store, revision_ref)?;
    let (_capsule_id, capsule) = load_default_capsule(store, &revision_id, &revision)?;
    let ref_events = revision_ref_events(store, &revision_id)?;
    let first_seen = ref_events
        .iter()
        .filter_map(|event| {
            event["timestamp_ms"]
                .as_u64()
                .map(|timestamp| (timestamp, event["ref"].as_str().unwrap_or("").to_string()))
        })
        .min_by_key(|(timestamp, _)| *timestamp);
    let first_seen_at_ms = first_seen.as_ref().map(|(timestamp, _)| *timestamp);
    let first_seen_ref = first_seen.map(|(_, ref_name)| ref_name);
    let policy_ids = if policies.is_empty() {
        store
            .list_refs("policies")?
            .into_iter()
            .filter_map(|(name, _)| name.strip_prefix("policies/").map(str::to_string))
            .collect::<Vec<_>>()
    } else {
        policies
    };

    let context = PolicyContext {
        revision_id: Some(revision_id),
        signer_agent_ids: signer_agents,
        signer_key_ids: signer_keys,
        touched_paths,
        trust_score: derive_capsule_trust_score(&capsule),
        now_ms: Some(current_time_ms()),
    };
    let mut results = Vec::new();
    for policy_id in &policy_ids {
        let (policy_ref, policy_object, policy) = load_policy(store, policy_id)?;
        let evaluation = evaluate_policy(&policy, &revision, &capsule, &context);
        let policy_first_seen = first_ref_event(store, &policy_ref)?;
        let policy_first_seen_at_ms = policy_first_seen
            .as_ref()
            .map(|(timestamp, _ref_name)| *timestamp);
        let policy_first_seen_ref = policy_first_seen.map(|(_timestamp, ref_name)| ref_name);
        let allowed_since_ms = if evaluation.is_ok() {
            observed_allowed_since(first_seen_at_ms, policy_first_seen_at_ms)
        } else {
            None
        };
        results.push(serde_json::json!({
            "policy_id": policy.policy_id,
            "ref": policy_ref,
            "object": policy_object.to_hex(),
            "allowed": evaluation.is_ok(),
            "policy_first_seen_at_ms": policy_first_seen_at_ms,
            "policy_first_seen_ref": policy_first_seen_ref,
            "allowed_since_ms": allowed_since_ms,
            "allowed_since_basis": {
                "revision_first_seen_at_ms": first_seen_at_ms,
                "policy_first_seen_at_ms": policy_first_seen_at_ms,
            },
            "reason": evaluation.err().map(|err| err.to_string()),
        }));
    }
    let allowed_by = results
        .iter()
        .filter(|result| result["allowed"].as_bool().unwrap_or(false))
        .count();
    let allowed_by_policy_ids = results
        .iter()
        .filter(|result| result["allowed"].as_bool().unwrap_or(false))
        .filter_map(|result| result["policy_id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let denied_by_policy_ids = results
        .iter()
        .filter(|result| !result["allowed"].as_bool().unwrap_or(false))
        .filter_map(|result| result["policy_id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let first_allowed = results
        .iter()
        .filter(|result| result["allowed"].as_bool().unwrap_or(false))
        .filter_map(|result| {
            result["allowed_since_ms"].as_u64().map(|timestamp| {
                (
                    timestamp,
                    result["policy_id"].as_str().unwrap_or("").to_string(),
                    result["ref"].as_str().unwrap_or("").to_string(),
                )
            })
        })
        .min_by_key(|(timestamp, _, _)| *timestamp);
    let first_allowed_at_ms = first_allowed
        .as_ref()
        .map(|(timestamp, _policy_id, _policy_ref)| *timestamp);
    let first_allowed_by_policy_id = first_allowed
        .as_ref()
        .map(|(_timestamp, policy_id, _policy_ref)| policy_id.clone())
        .or_else(|| allowed_by_policy_ids.first().cloned());
    let first_allowed_policy_ref = first_allowed
        .as_ref()
        .map(|(_timestamp, _policy_id, policy_ref)| policy_ref.clone());
    let answer = allowed_answer(
        &allowed_by_policy_ids,
        &denied_by_policy_ids,
        first_allowed_by_policy_id.as_deref(),
        first_allowed_policy_ref.as_deref(),
        first_allowed_at_ms,
        first_seen_at_ms,
        first_seen_ref.as_deref(),
    );

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "timeline.allowed",
                "revision": revision_id.to_hex(),
                "allowed": allowed_by > 0,
                "allowed_by": allowed_by,
                "allowed_by_policy_ids": allowed_by_policy_ids,
                "denied_by_policy_ids": denied_by_policy_ids,
                "first_allowed_by_policy_id": first_allowed_by_policy_id,
                "first_allowed_policy_ref": first_allowed_policy_ref,
                "first_seen_at_ms": first_seen_at_ms,
                "first_seen_ref": first_seen_ref,
                "first_allowed_at_ms": first_allowed_at_ms,
                "answer": answer,
                "ref_events": ref_events,
                "policies": results,
            }))?
        );
    } else {
        println!(
            "Revision {} is allowed by {} current polic{}.",
            revision_id.to_hex(),
            allowed_by,
            if allowed_by == 1 { "y" } else { "ies" }
        );
        if let Some(first_seen_at_ms) = first_seen_at_ms {
            println!(
                "  First seen: {} via {}",
                first_seen_at_ms,
                first_seen_ref.as_deref().unwrap_or("unknown ref")
            );
        }
        if let Some(first_allowed_at_ms) = first_allowed_at_ms {
            println!(
                "  First allowed by current policy set: {} via {}",
                first_allowed_at_ms,
                first_allowed_by_policy_id
                    .as_deref()
                    .unwrap_or("unknown policy")
            );
        }
        println!(
            "  Policy that let this through: {}",
            answer["which_policy_let_this_through"]
                .as_str()
                .unwrap_or("unknown")
        );
        for result in &results {
            let verdict = if result["allowed"].as_bool().unwrap_or(false) {
                "allow"
            } else {
                "deny"
            };
            println!(
                "  {} {} ({})",
                verdict,
                result["policy_id"].as_str().unwrap_or(""),
                result["reason"].as_str().unwrap_or("ok")
            );
        }
    }
    Ok(())
}

fn allowed_answer(
    allowed_policy_ids: &[String],
    denied_policy_ids: &[String],
    first_allowed_policy_id: Option<&str>,
    first_allowed_policy_ref: Option<&str>,
    first_allowed_at_ms: Option<u64>,
    first_seen_at_ms: Option<u64>,
    first_seen_ref: Option<&str>,
) -> serde_json::Value {
    let allowed_now = !allowed_policy_ids.is_empty();
    let first_allowed_by_policy_id = allowed_policy_ids.first().cloned();
    let which_policy = if allowed_policy_ids.is_empty() {
        "none".to_string()
    } else {
        allowed_policy_ids.join(", ")
    };
    let when = match (allowed_now, first_allowed_at_ms, first_seen_at_ms) {
        (true, Some(timestamp), _) => {
            format!("Revision is allowed under the current policy set from observed ref event {timestamp}.")
        }
        (true, None, _) => {
            "Revision is allowed under the current policy set, but no reflog event proves when it first appeared.".to_string()
        }
        (false, _, Some(timestamp)) => {
            format!("Revision first appeared at observed ref event {timestamp}, but no selected current policy allows it.")
        }
        (false, _, None) => {
            "No selected current policy allows this revision, and no reflog event proves when it first appeared.".to_string()
        }
    };

    serde_json::json!({
        "allowed_now": allowed_now,
        "scope": "current_policy_set",
        "when_did_this_become_allowed": when,
        "which_policy_let_this_through": which_policy,
        "first_allowed_by_policy_id": first_allowed_policy_id.or(first_allowed_by_policy_id.as_deref()),
        "first_allowed_policy_ref": first_allowed_policy_ref,
        "allowed_policy_ids": allowed_policy_ids,
        "denied_policy_ids": denied_policy_ids,
        "first_allowed_at_ms": first_allowed_at_ms,
        "first_seen_at_ms": first_seen_at_ms,
        "first_seen_ref": first_seen_ref,
        "note": "Policy object history is not replayed; allowed timing is derived from the earliest observed ref events for this revision and the selected current policy refs.",
    })
}

fn observed_allowed_since(
    revision_first_seen_at_ms: Option<u64>,
    policy_first_seen_at_ms: Option<u64>,
) -> Option<u64> {
    match (revision_first_seen_at_ms, policy_first_seen_at_ms) {
        (Some(revision), Some(policy)) => Some(revision.max(policy)),
        (Some(revision), None) => Some(revision),
        (None, Some(policy)) => Some(policy),
        (None, None) => None,
    }
}

fn first_ref_event(store: &ClawStore, ref_name: &str) -> anyhow::Result<Option<(u64, String)>> {
    Ok(reflog::read_reflog(store.layout(), ref_name)?
        .into_iter()
        .map(|entry| (entry.timestamp_ms, ref_name.to_string()))
        .min_by_key(|(timestamp, _)| *timestamp))
}

fn revision_ref_events(
    store: &ClawStore,
    revision_id: &claw_core::id::ObjectId,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut events = Vec::new();
    for (ref_name, _) in store.list_refs("")? {
        let Ok(entries) = reflog::read_reflog(store.layout(), &ref_name) else {
            continue;
        };
        for entry in entries {
            if entry.new == *revision_id {
                events.push(serde_json::json!({
                    "ref": ref_name,
                    "old": entry.old.to_hex(),
                    "new": entry.new.to_hex(),
                    "timestamp_ms": entry.timestamp_ms,
                    "author": entry.author,
                    "message": entry.message,
                }));
            }
        }
    }
    events.sort_by_key(|event| event["timestamp_ms"].as_u64().unwrap_or(u64::MAX));
    Ok(events)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{allowed_answer, observed_allowed_since, TimelineArgs, TimelineCommand};

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: TimelineArgs,
    }

    #[test]
    fn parses_timeline_ref_default() {
        let cli = TestCli::parse_from(["claw", "ref"]);
        match cli.args.command {
            TimelineCommand::Ref { ref_name } => assert_eq!(ref_name, "heads/main"),
            _ => panic!("expected ref command"),
        }
    }

    #[test]
    fn parses_timeline_allowed_json() {
        let cli = TestCli::parse_from([
            "claw",
            "--json",
            "allowed",
            "--revision",
            "heads/main",
            "--policy",
            "release",
        ]);
        assert!(cli.args.json);
        match cli.args.command {
            TimelineCommand::Allowed {
                revision, policies, ..
            } => {
                assert_eq!(revision, "heads/main");
                assert_eq!(policies, vec!["release"]);
            }
            _ => panic!("expected allowed command"),
        }
    }

    #[test]
    fn allowed_answer_names_policy_and_current_scope() {
        let allowed = vec!["release".to_string(), "security".to_string()];
        let denied = vec!["legacy".to_string()];
        let answer = allowed_answer(
            &allowed,
            &denied,
            Some("release"),
            Some("refs/policies/release"),
            Some(1_700),
            Some(1_700),
            Some("heads/main"),
        );

        assert_eq!(answer["allowed_now"], true);
        assert_eq!(answer["scope"], "current_policy_set");
        assert_eq!(answer["first_allowed_by_policy_id"], "release");
        assert_eq!(answer["first_allowed_policy_ref"], "refs/policies/release");
        assert_eq!(answer["which_policy_let_this_through"], "release, security");
        assert_eq!(answer["first_allowed_at_ms"], 1_700);
        assert!(answer["when_did_this_become_allowed"]
            .as_str()
            .unwrap()
            .contains("current policy set"));
    }

    #[test]
    fn allowed_answer_explains_denial_without_policy() {
        let answer = allowed_answer(
            &[],
            &["release".to_string()],
            None,
            None,
            None,
            Some(2_000),
            Some("heads/main"),
        );

        assert_eq!(answer["allowed_now"], false);
        assert_eq!(answer["which_policy_let_this_through"], "none");
        assert!(answer["first_allowed_by_policy_id"].is_null());
        assert!(answer["when_did_this_become_allowed"]
            .as_str()
            .unwrap()
            .contains("no selected current policy allows it"));
    }

    #[test]
    fn observed_allowed_since_uses_latest_observed_input() {
        assert_eq!(
            observed_allowed_since(Some(1_000), Some(1_500)),
            Some(1_500)
        );
        assert_eq!(
            observed_allowed_since(Some(2_000), Some(1_500)),
            Some(2_000)
        );
        assert_eq!(observed_allowed_since(Some(2_000), None), Some(2_000));
        assert_eq!(observed_allowed_since(None, Some(1_500)), Some(1_500));
        assert_eq!(observed_allowed_since(None, None), None);
    }
}
