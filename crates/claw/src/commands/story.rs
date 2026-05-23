use clap::{Args, Subcommand};

use claw_core::id::ChangeId;
use claw_core::object::Object;
use claw_core::types::{Change, Intent, Revision};
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::object_refs::load_default_capsule;

#[derive(Args)]
pub struct StoryArgs {
    #[command(subcommand)]
    command: StoryCommand,
}

#[derive(Subcommand)]
enum StoryCommand {
    /// Export an intent/change history as a human-readable audit narrative
    Export {
        /// Intent ID (ULID)
        #[arg(long)]
        intent: String,
        /// Output format: markdown|json
        #[arg(long, default_value = "markdown")]
        format: String,
    },
}

pub fn run(args: StoryArgs) -> anyhow::Result<()> {
    match args.command {
        StoryCommand::Export { intent, format } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let (intent, object_id) = load_intent(&store, &intent)?;
            let story = build_story(&store, &intent, object_id)?;
            if format.eq_ignore_ascii_case("json") {
                println!("{}", serde_json::to_string_pretty(&story)?);
            } else if format.eq_ignore_ascii_case("markdown") {
                print_markdown_story(&story);
            } else {
                anyhow::bail!("unknown story format '{format}'; expected markdown|json");
            }
        }
    }

    Ok(())
}

fn load_intent(store: &ClawStore, id: &str) -> anyhow::Result<(Intent, claw_core::id::ObjectId)> {
    let ref_name = format!("intents/{id}");
    let object_id = store
        .get_ref(&ref_name)?
        .ok_or_else(|| anyhow::anyhow!("intent not found: {id}"))?;
    match store.load_object(&object_id)? {
        Object::Intent(intent) => Ok((intent, object_id)),
        _ => anyhow::bail!("ref does not point to an intent object: {ref_name}"),
    }
}

fn build_story(
    store: &ClawStore,
    intent: &Intent,
    object_id: claw_core::id::ObjectId,
) -> anyhow::Result<serde_json::Value> {
    let mut changes = Vec::new();
    for change_id in &intent.change_ids {
        changes.push(match load_change(store, change_id) {
            Ok((change_object_id, change)) => change_story(store, change_object_id, &change)?,
            Err(err) => serde_json::json!({
                "id": change_id,
                "error": err.to_string(),
            }),
        });
    }
    let summary = story_summary(intent, &changes);
    let narrative = story_narrative(intent, &summary, &changes);

    Ok(serde_json::json!({
        "schema_version": 1,
        "action": "story.export",
        "summary": summary,
        "narrative": narrative,
        "intent": {
            "id": intent.id.to_string(),
            "object": object_id.to_hex(),
            "title": intent.title,
            "goal": intent.goal,
            "status": format!("{:?}", intent.status).to_ascii_lowercase(),
            "constraints": intent.constraints,
            "acceptance_tests": intent.acceptance_tests,
            "links": intent.links,
            "policies": intent.policy_refs,
            "agents": intent.agents,
            "depends_on": intent.depends_on,
            "supersedes": intent.supersedes,
            "created_at_ms": intent.created_at_ms,
            "updated_at_ms": intent.updated_at_ms,
        },
        "changes": changes,
    }))
}

fn story_summary(intent: &Intent, changes: &[serde_json::Value]) -> serde_json::Value {
    let revision_count = changes
        .iter()
        .filter(|change| {
            let revision = &change["head_revision"];
            !revision.is_null() && revision["missing"].as_bool() != Some(true)
        })
        .count();
    let mut capsule_count = 0usize;
    let mut missing_capsule_count = 0usize;
    let mut evidence_count = 0usize;
    let mut passing_evidence_count = 0usize;
    let mut failing_evidence_count = 0usize;
    let mut signature_count = 0usize;
    let mut unsigned_capsule_count = 0usize;
    let mut private_capsule_count = 0usize;

    for change in changes {
        let capsule = &change["head_revision"]["capsule"];
        if capsule.is_null() {
            continue;
        }
        if capsule["missing"].as_bool().unwrap_or(false) {
            missing_capsule_count += 1;
            continue;
        }
        capsule_count += 1;
        let signatures = capsule["signature_count"].as_u64().unwrap_or(0) as usize;
        signature_count += signatures;
        if signatures == 0 {
            unsigned_capsule_count += 1;
        }
        if capsule["has_private_fields"].as_bool().unwrap_or(false) {
            private_capsule_count += 1;
        }
        if let Some(items) = capsule["evidence"].as_array() {
            evidence_count += items.len();
            passing_evidence_count += items
                .iter()
                .filter(|item| {
                    item["status"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("pass"))
                })
                .count();
            failing_evidence_count += items
                .iter()
                .filter(|item| {
                    !item["status"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("pass"))
                })
                .count();
        }
    }

    let trust_posture = if missing_capsule_count > 0 {
        "incomplete_capsules"
    } else if failing_evidence_count > 0 {
        "evidence_attention_required"
    } else if capsule_count > 0 && evidence_count > 0 {
        "evidence_passed"
    } else {
        "insufficient_evidence"
    };

    serde_json::json!({
        "change_count": changes.len(),
        "revision_count": revision_count,
        "capsule_count": capsule_count,
        "missing_capsule_count": missing_capsule_count,
        "signature_count": signature_count,
        "unsigned_capsule_count": unsigned_capsule_count,
        "private_capsule_count": private_capsule_count,
        "evidence_count": evidence_count,
        "passing_evidence_count": passing_evidence_count,
        "failing_evidence_count": failing_evidence_count,
        "policy_count": intent.policy_refs.len(),
        "acceptance_test_count": intent.acceptance_tests.len(),
        "agent_count": intent.agents.len(),
        "trust_posture": trust_posture,
    })
}

fn story_narrative(
    intent: &Intent,
    summary: &serde_json::Value,
    changes: &[serde_json::Value],
) -> serde_json::Value {
    let trust_posture = summary["trust_posture"].as_str().unwrap_or("unknown");
    let audit_verdict = match trust_posture {
        "evidence_passed" => "ready_for_review",
        "evidence_attention_required" => "evidence_attention_required",
        "incomplete_capsules" => "incomplete_provenance",
        "insufficient_evidence" => "insufficient_evidence",
        _ => "needs_review",
    };
    let audience_summary = format!(
        "Intent '{}' is {} with {} linked change(s), {} revision(s), and trust posture '{}'.",
        intent.title,
        format!("{:?}", intent.status).to_ascii_lowercase(),
        summary["change_count"].as_u64().unwrap_or(0),
        summary["revision_count"].as_u64().unwrap_or(0),
        trust_posture
    );
    let risk_notes = story_risk_notes(intent, summary);
    let next_actions = story_next_actions(intent, summary, &risk_notes);
    let timeline = changes
        .iter()
        .enumerate()
        .map(|(index, change)| story_timeline_entry(index + 1, change))
        .collect::<Vec<_>>();

    serde_json::json!({
        "audience_summary": audience_summary,
        "audit_verdict": audit_verdict,
        "risk_notes": risk_notes,
        "next_actions": next_actions,
        "timeline": timeline,
    })
}

fn story_risk_notes(intent: &Intent, summary: &serde_json::Value) -> Vec<String> {
    let mut notes = Vec::new();
    let missing_capsules = summary["missing_capsule_count"].as_u64().unwrap_or(0);
    if missing_capsules > 0 {
        notes.push(format!(
            "{missing_capsules} linked change(s) have revisions without attached capsules."
        ));
    }
    let failing_evidence = summary["failing_evidence_count"].as_u64().unwrap_or(0);
    if failing_evidence > 0 {
        notes.push(format!(
            "{failing_evidence} evidence item(s) require attention."
        ));
    }
    let unsigned_capsules = summary["unsigned_capsule_count"].as_u64().unwrap_or(0);
    if unsigned_capsules > 0 {
        notes.push(format!(
            "{unsigned_capsules} capsule(s) do not contain signatures."
        ));
    }
    let private_capsules = summary["private_capsule_count"].as_u64().unwrap_or(0);
    if private_capsules > 0 {
        notes.push(format!(
            "{private_capsules} capsule(s) include encrypted private fields; authorized recipients may be needed for full review."
        ));
    }
    if intent.policy_refs.is_empty() {
        notes.push("No policies are linked to this intent.".to_string());
    }
    if intent.acceptance_tests.is_empty() {
        notes.push("No executable acceptance tests are linked to this intent.".to_string());
    }
    if notes.is_empty() {
        notes.push("No obvious provenance gaps were found in the exported story.".to_string());
    }
    notes
}

fn story_next_actions(
    intent: &Intent,
    summary: &serde_json::Value,
    risk_notes: &[String],
) -> Vec<String> {
    let mut actions = Vec::new();
    if summary["missing_capsule_count"].as_u64().unwrap_or(0) > 0 {
        actions.push(
            "Attach or repair missing revision capsules before relying on this history."
                .to_string(),
        );
    }
    if summary["failing_evidence_count"].as_u64().unwrap_or(0) > 0 {
        actions.push("Review failing evidence and rerun or replace stale checks.".to_string());
    }
    if intent.policy_refs.is_empty() {
        actions.push("Attach the intended release or review policy to the intent.".to_string());
    }
    if intent.acceptance_tests.is_empty() {
        actions.push(
            "Add executable acceptance tests so future exports cite runnable specs.".to_string(),
        );
    }
    if actions.is_empty()
        && risk_notes
            .iter()
            .any(|note| note.starts_with("No obvious provenance gaps"))
    {
        actions.push("Use the linked changes, capsules, and evidence as the audit trail for review or handoff.".to_string());
    }
    actions
}

fn story_timeline_entry(sequence: usize, change: &serde_json::Value) -> serde_json::Value {
    let change_id = change["id"].as_str().unwrap_or("unknown");
    let change_status = change["status"].as_str().unwrap_or("unknown");
    let revision = &change["head_revision"];
    if revision.is_null() {
        return serde_json::json!({
            "sequence": sequence,
            "change_id": change_id,
            "change_status": change_status,
            "revision": serde_json::Value::Null,
            "capsule": serde_json::Value::Null,
            "evidence_passed": 0,
            "evidence_failed": 0,
            "narrative": format!("Change {change_id} is {change_status} and has no head revision."),
        });
    }

    let capsule = &revision["capsule"];
    let evidence = capsule["evidence"].as_array().cloned().unwrap_or_default();
    let evidence_passed = evidence
        .iter()
        .filter(|item| {
            item["status"]
                .as_str()
                .is_some_and(|status| status.eq_ignore_ascii_case("pass"))
        })
        .count();
    let evidence_failed = evidence.len().saturating_sub(evidence_passed);
    let revision_hex = revision["hex"].as_str().unwrap_or("");
    let capsule_id = if capsule["missing"].as_bool().unwrap_or(false) {
        None
    } else {
        capsule["id"].as_str()
    };
    let narrative = if let Some(capsule_id) = capsule_id {
        format!(
            "Change {change_id} points at revision {revision_hex} with capsule {capsule_id}; {evidence_passed} evidence item(s) passed and {evidence_failed} need attention."
        )
    } else {
        format!("Change {change_id} points at revision {revision_hex}, but its capsule is missing.")
    };

    serde_json::json!({
        "sequence": sequence,
        "change_id": change_id,
        "change_status": change_status,
        "revision": revision_hex,
        "revision_summary": revision["summary"].as_str().unwrap_or(""),
        "capsule": capsule_id,
        "agent_id": capsule["agent_id"].as_str(),
        "evidence_passed": evidence_passed,
        "evidence_failed": evidence_failed,
        "narrative": narrative,
    })
}

fn change_story(
    store: &ClawStore,
    object_id: claw_core::id::ObjectId,
    change: &Change,
) -> anyhow::Result<serde_json::Value> {
    let revision = match change.head_revision {
        Some(revision_id) => revision_story(store, revision_id)?,
        None => serde_json::json!(null),
    };

    Ok(serde_json::json!({
        "id": change.id.to_string(),
        "object": object_id.to_hex(),
        "status": format!("{:?}", change.status).to_ascii_lowercase(),
        "head_revision": revision,
        "created_at_ms": change.created_at_ms,
        "updated_at_ms": change.updated_at_ms,
    }))
}

fn revision_story(
    store: &ClawStore,
    revision_id: claw_core::id::ObjectId,
) -> anyhow::Result<serde_json::Value> {
    let Object::Revision(revision) = store.load_object(&revision_id)? else {
        anyhow::bail!("head revision is not a revision: {}", revision_id.to_hex());
    };
    Ok(revision_json(store, revision_id, &revision))
}

fn revision_json(
    store: &ClawStore,
    revision_id: claw_core::id::ObjectId,
    revision: &Revision,
) -> serde_json::Value {
    let capsule = match load_default_capsule(store, &revision_id, revision) {
        Ok((capsule_id, capsule)) => serde_json::json!({
            "id": capsule_id.to_hex(),
            "agent_id": capsule.public_fields.agent_id,
            "evidence": capsule.public_fields.evidence,
            "signature_count": capsule.signatures.len(),
            "has_private_fields": capsule.encrypted_private.is_some(),
        }),
        Err(err) => serde_json::json!({
            "missing": true,
            "reason": err.to_string(),
        }),
    };

    serde_json::json!({
        "id": revision_id.to_string(),
        "hex": revision_id.to_hex(),
        "summary": revision.summary,
        "author": revision.author,
        "created_at_ms": revision.created_at_ms,
        "parents": revision.parents.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
        "patches": revision.patches.iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
        "capsule": capsule,
    })
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

fn print_markdown_story(story: &serde_json::Value) {
    let intent = &story["intent"];
    let summary = &story["summary"];
    let narrative = &story["narrative"];
    println!("# Intent Story: {}", intent["title"].as_str().unwrap_or(""));
    println!();
    println!("- Intent: `{}`", intent["id"].as_str().unwrap_or(""));
    println!("- Status: `{}`", intent["status"].as_str().unwrap_or(""));
    println!("- Object: `{}`", intent["object"].as_str().unwrap_or(""));
    if let Some(goal) = intent["goal"].as_str().filter(|goal| !goal.is_empty()) {
        println!();
        println!("## Goal");
        println!();
        println!("{goal}");
    }

    println!();
    println!("## Executive Summary");
    println!();
    println!(
        "- Changes: {}",
        summary["change_count"].as_u64().unwrap_or(0)
    );
    println!(
        "- Revisions: {}",
        summary["revision_count"].as_u64().unwrap_or(0)
    );
    println!(
        "- Capsules: {} present, {} missing",
        summary["capsule_count"].as_u64().unwrap_or(0),
        summary["missing_capsule_count"].as_u64().unwrap_or(0)
    );
    println!(
        "- Evidence: {} passed, {} attention, {} total",
        summary["passing_evidence_count"].as_u64().unwrap_or(0),
        summary["failing_evidence_count"].as_u64().unwrap_or(0),
        summary["evidence_count"].as_u64().unwrap_or(0)
    );
    println!(
        "- Trust posture: `{}`",
        summary["trust_posture"].as_str().unwrap_or("unknown")
    );

    println!();
    println!("## Audit Narrative");
    println!();
    println!(
        "{}",
        narrative["audience_summary"].as_str().unwrap_or_default()
    );
    println!();
    println!(
        "- Audit verdict: `{}`",
        narrative["audit_verdict"]
            .as_str()
            .unwrap_or("needs_review")
    );
    print_markdown_string_values("Risk notes", &narrative["risk_notes"]);
    print_markdown_string_values("Next actions", &narrative["next_actions"]);
    print_story_timeline(&narrative["timeline"]);

    print_string_list("Acceptance Tests", &intent["acceptance_tests"]);
    print_string_list("Policies", &intent["policies"]);
    print_string_list("Agents", &intent["agents"]);

    println!();
    println!("## Changes");
    let changes = story["changes"].as_array().cloned().unwrap_or_default();
    if changes.is_empty() {
        println!();
        println!("No changes are linked to this intent.");
        return;
    }

    for change in changes {
        println!();
        println!(
            "### Change `{}`",
            change["id"].as_str().unwrap_or("unknown")
        );
        println!();
        println!("- Status: `{}`", change["status"].as_str().unwrap_or(""));
        let revision = &change["head_revision"];
        if revision.is_null() {
            println!("- Revision: none");
            continue;
        }
        println!("- Revision: `{}`", revision["hex"].as_str().unwrap_or(""));
        println!("- Summary: {}", revision["summary"].as_str().unwrap_or(""));
        println!("- Author: `{}`", revision["author"].as_str().unwrap_or(""));
        println!(
            "- Patches: {}",
            revision["patches"].as_array().map_or(0, Vec::len)
        );

        let capsule = &revision["capsule"];
        if capsule["missing"].as_bool().unwrap_or(false) {
            println!(
                "- Capsule: missing ({})",
                capsule["reason"].as_str().unwrap_or("not found")
            );
        } else {
            println!("- Capsule: `{}`", capsule["id"].as_str().unwrap_or(""));
            println!("- Agent: `{}`", capsule["agent_id"].as_str().unwrap_or(""));
            println!(
                "- Evidence items: {}",
                capsule["evidence"].as_array().map_or(0, Vec::len)
            );
            println!(
                "- Signatures: {}",
                capsule["signature_count"].as_u64().unwrap_or(0)
            );
            print_evidence_items(&capsule["evidence"]);
        }
    }
}

fn print_story_timeline(value: &serde_json::Value) {
    let items = value.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return;
    }
    println!();
    println!("### Timeline");
    println!();
    for item in items {
        println!(
            "{}. {}",
            item["sequence"].as_u64().unwrap_or(0),
            item["narrative"].as_str().unwrap_or("")
        );
    }
}

fn print_evidence_items(value: &serde_json::Value) {
    let items = value.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return;
    }
    println!();
    println!("Evidence:");
    for item in items {
        let name = item["name"].as_str().unwrap_or("unnamed");
        let status = item["status"].as_str().unwrap_or("unknown");
        let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
        println!("- `{name}` = `{status}` ({duration_ms} ms)");
        if let Some(command) = item["command"].as_str().filter(|value| !value.is_empty()) {
            println!("  - command: `{command}`");
        }
        if let Some(runner) = item["runner_identity"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            println!("  - runner: `{runner}`");
        }
        if let Some(summary) = item["summary"].as_str().filter(|value| !value.is_empty()) {
            println!("  - summary: {summary}");
        }
    }
}

fn print_markdown_string_values(title: &str, value: &serde_json::Value) {
    let items = value.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return;
    }
    println!();
    println!("### {title}");
    println!();
    for item in items {
        println!("- {}", item.as_str().unwrap_or(""));
    }
}

fn print_string_list(title: &str, value: &serde_json::Value) {
    let items = value.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return;
    }
    println!();
    println!("## {title}");
    println!();
    for item in items {
        println!("- {}", item.as_str().unwrap_or(""));
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{story_narrative, story_summary, StoryArgs, StoryCommand};
    use claw_core::id::IntentId;
    use claw_core::types::{Intent, IntentStatus};

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: StoryArgs,
    }

    #[test]
    fn parses_story_export() {
        let cli = TestCli::parse_from([
            "claw",
            "export",
            "--intent",
            "01H00000000000000000000000",
            "--format",
            "json",
        ]);

        match cli.args.command {
            StoryCommand::Export { intent, format } => {
                assert_eq!(intent, "01H00000000000000000000000");
                assert_eq!(format, "json");
            }
        }
    }

    #[test]
    fn summary_classifies_evidence_and_missing_capsules() {
        let intent = Intent {
            id: IntentId::new(),
            title: "Audit".to_string(),
            goal: "Explain history".to_string(),
            constraints: vec![],
            acceptance_tests: vec!["cargo test".to_string()],
            links: vec![],
            policy_refs: vec!["release".to_string()],
            agents: vec!["agent-a".to_string()],
            change_ids: vec![],
            depends_on: vec![],
            supersedes: vec![],
            status: IntentStatus::Open,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        let changes = vec![
            serde_json::json!({
                "head_revision": {
                    "capsule": {
                        "evidence": [
                            {"name": "test", "status": "pass"},
                            {"name": "lint", "status": "fail"}
                        ]
                    }
                }
            }),
            serde_json::json!({
                "head_revision": {
                    "capsule": {
                        "missing": true,
                        "reason": "not found"
                    }
                }
            }),
        ];

        let summary = story_summary(&intent, &changes);

        assert_eq!(summary["change_count"], 2);
        assert_eq!(summary["revision_count"], 2);
        assert_eq!(summary["capsule_count"], 1);
        assert_eq!(summary["missing_capsule_count"], 1);
        assert_eq!(summary["signature_count"], 0);
        assert_eq!(summary["unsigned_capsule_count"], 1);
        assert_eq!(summary["private_capsule_count"], 0);
        assert_eq!(summary["evidence_count"], 2);
        assert_eq!(summary["passing_evidence_count"], 1);
        assert_eq!(summary["failing_evidence_count"], 1);
        assert_eq!(summary["policy_count"], 1);
        assert_eq!(summary["acceptance_test_count"], 1);
        assert_eq!(summary["agent_count"], 1);
        assert_eq!(summary["trust_posture"], "incomplete_capsules");

        let narrative = story_narrative(&intent, &summary, &changes);
        assert_eq!(narrative["audit_verdict"], "incomplete_provenance");
        assert!(narrative["audience_summary"]
            .as_str()
            .unwrap()
            .contains("Intent 'Audit'"));
        assert!(narrative["risk_notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note
                .as_str()
                .unwrap_or_default()
                .contains("without attached capsules")));
        assert_eq!(narrative["timeline"].as_array().unwrap().len(), 2);
    }
}
