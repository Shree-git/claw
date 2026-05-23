use clap::{Args, Subcommand};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use claw_core::id::IntentId;
use claw_core::object::Object;
use claw_core::types::{ChangeStatus, Intent, IntentStatus};
use claw_store::ClawStore;

use super::object_refs::load_default_capsule;
use crate::config::find_repo_root;

#[derive(Args)]
pub struct IntentArgs {
    /// Output result as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: IntentCommand,
}

#[derive(Subcommand)]
enum IntentCommand {
    /// Create a new intent
    #[command(alias = "create")]
    New {
        /// Intent title
        #[arg(short, long)]
        title: String,
        /// Intent goal
        #[arg(short, long, default_value = "")]
        goal: String,
        /// Runnable acceptance command or spec reference. Repeatable.
        #[arg(long = "acceptance-test")]
        acceptance_tests: Vec<String>,
    },
    /// Show an intent
    Show {
        /// Intent ID (ULID)
        id: String,
    },
    /// List intents
    List,
    /// Export an intent/change graph
    Graph {
        /// Output format: mermaid|json|html
        #[arg(long, default_value = "mermaid")]
        format: String,
    },
    /// Run executable acceptance tests linked to an intent
    RunAcceptance {
        /// Intent ID (ULID)
        id: String,
        /// Stop after this many milliseconds per acceptance command
        #[arg(long, default_value_t = 300_000)]
        timeout_ms: u64,
        /// Continue running remaining acceptance commands after a failure
        #[arg(long)]
        keep_going: bool,
    },
    /// Update an intent
    Update {
        /// Intent ID (ULID)
        id: String,
        /// New status
        #[arg(short, long)]
        status: Option<String>,
    },
    /// Manage policy references attached to an intent
    Policy {
        #[command(subcommand)]
        command: IntentPolicyCommand,
    },
}

#[derive(Subcommand)]
enum IntentPolicyCommand {
    /// List policy references attached to an intent
    List {
        /// Intent ID (ULID)
        id: String,
    },
    /// Attach a policy reference to an intent
    Add {
        /// Intent ID (ULID)
        id: String,
        /// Policy ID or ref, for example `ci-required` or `policies/ci-required`
        policy_ref: String,
        /// Validate and print the planned change without writing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a policy reference from an intent
    Remove {
        /// Intent ID (ULID)
        id: String,
        /// Policy ID or ref to remove
        policy_ref: String,
        /// Validate and print the planned change without writing it
        #[arg(long)]
        dry_run: bool,
    },
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use clap::Parser;

    use super::{
        add_policy_ref, normalized_policy_ref, remove_policy_ref, render_graph_html,
        AcceptanceOutcome, IntentArgs, IntentCommand, IntentPolicyCommand,
    };

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: IntentArgs,
    }

    #[test]
    fn parse_create_alias_as_new() {
        let cli = TestCli::parse_from(["claw", "create", "--title", "hello"]);

        match cli.args.command {
            IntentCommand::New {
                title,
                goal,
                acceptance_tests,
            } => {
                assert_eq!(title, "hello");
                assert_eq!(goal, "");
                assert!(acceptance_tests.is_empty());
            }
            _ => panic!("expected new command"),
        }
    }

    #[test]
    fn parse_policy_add_dry_run() {
        let cli = TestCli::parse_from([
            "claw",
            "policy",
            "add",
            "01H00000000000000000000000",
            "ci-required",
            "--dry-run",
        ]);

        match cli.args.command {
            IntentCommand::Policy {
                command:
                    IntentPolicyCommand::Add {
                        id,
                        policy_ref,
                        dry_run,
                    },
            } => {
                assert_eq!(id, "01H00000000000000000000000");
                assert_eq!(policy_ref, "ci-required");
                assert!(dry_run);
            }
            _ => panic!("expected policy add command"),
        }
    }

    #[test]
    fn policy_ref_helpers_are_idempotent() {
        let mut refs = Vec::new();

        assert!(add_policy_ref(&mut refs, "ci-required".to_string()));
        assert!(!add_policy_ref(&mut refs, "ci-required".to_string()));
        assert_eq!(refs, vec!["ci-required"]);
        assert!(remove_policy_ref(&mut refs, "ci-required"));
        assert!(!remove_policy_ref(&mut refs, "ci-required"));
        assert!(refs.is_empty());
    }

    #[test]
    fn policy_ref_normalization_strips_repo_prefix() {
        assert_eq!(
            normalized_policy_ref("policies/ci-required").unwrap(),
            "ci-required"
        );
        assert_eq!(
            normalized_policy_ref(" ci-required ").unwrap(),
            "ci-required"
        );
        assert!(normalized_policy_ref("  ").is_err());
    }

    #[test]
    fn parses_run_acceptance_timeout() {
        let cli = TestCli::parse_from([
            "claw",
            "run-acceptance",
            "01H00000000000000000000000",
            "--timeout-ms",
            "5000",
            "--keep-going",
        ]);

        match cli.args.command {
            IntentCommand::RunAcceptance {
                id,
                timeout_ms,
                keep_going,
            } => {
                assert_eq!(id, "01H00000000000000000000000");
                assert_eq!(timeout_ms, 5000);
                assert!(keep_going);
            }
            _ => panic!("expected run-acceptance command"),
        }
    }

    #[test]
    fn acceptance_outcome_maps_to_evidence_status() {
        assert_eq!(AcceptanceOutcome::Passed.status(), "pass");
        assert_eq!(AcceptanceOutcome::Failed.status(), "fail");
        assert_eq!(AcceptanceOutcome::TimedOut.status(), "timeout");
    }

    #[test]
    fn graph_html_embeds_nodes_edges_and_controls() {
        let html = render_graph_html(
            &[serde_json::json!({
                "id": "intent:01H00000000000000000000000",
                "type": "intent",
                "label": "Release gate",
                "status": "open",
            })],
            &[serde_json::json!({
                "from": "intent:01H00000000000000000000000",
                "to": "policy:release",
                "relation": "requires",
            })],
            1,
            0,
        )
        .unwrap();

        assert!(html.contains("<title>Claw Intent Graph</title>"));
        assert!(html.contains("const nodes = [{"));
        assert!(html.contains("Release gate"));
        assert!(html.contains("typeFilter"));
        assert!(html.contains("details"));
    }
}

pub fn run(args: IntentArgs) -> anyhow::Result<()> {
    let json = args.json;
    match args.command {
        IntentCommand::New {
            title,
            goal,
            acceptance_tests,
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
                acceptance_tests,
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

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "intent.create",
                        "created": true,
                        "intent": intent_json(&intent, Some(id.to_hex())),
                    }))?
                );
            } else {
                println!("Created intent: {}", intent.id);
                println!("  Title: {title}");
                println!("  Object: {id}");
            }
        }
        IntentCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let obj_id = store.get_ref(&format!("intents/{id}"))?.ok_or_else(|| {
                anyhow::anyhow!(
                    "intent not found: {id}. Run `claw intent list` to inspect available intents."
                )
            })?;
            let obj = store.load_object(&obj_id)?;
            if let Object::Intent(intent) = obj {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "intent.show",
                            "intent": intent_json(&intent, Some(obj_id.to_hex())),
                        }))?
                    );
                } else {
                    println!("Intent: {}", intent.id);
                    println!("  Title: {}", intent.title);
                    println!("  Status: {:?}", intent.status);
                    println!("  Goal: {}", intent.goal);
                    if !intent.acceptance_tests.is_empty() {
                        println!("  Acceptance tests:");
                        for test in &intent.acceptance_tests {
                            println!("    {test}");
                        }
                    }
                }
            }
        }
        IntentCommand::List => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("intents")?;
            if json {
                let intents = refs
                    .iter()
                    .filter_map(|(_name, id)| match store.load_object(id) {
                        Ok(Object::Intent(intent)) => Some(intent_json(&intent, Some(id.to_hex()))),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "intent.list",
                        "intent_count": intents.len(),
                        "intents": intents,
                    }))?
                );
            } else if refs.is_empty() {
                println!("No intents found.");
            } else {
                for (_name, id) in &refs {
                    if let Ok(Object::Intent(intent)) = store.load_object(id) {
                        println!("{} {:?} {}", intent.id, intent.status, intent.title);
                    }
                }
            }
        }
        IntentCommand::Graph { format } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            run_graph(&store, &format, json)?;
        }
        IntentCommand::RunAcceptance {
            id,
            timeout_ms,
            keep_going,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let (intent, object_id) = load_intent(&store, &id)?;
            run_acceptance(&root, &intent, object_id, timeout_ms, keep_going, json)?;
        }
        IntentCommand::Update { id, status } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let (mut intent, _) = load_intent(&store, &id)?;
            if let Some(s) = status {
                intent.status = match s.to_lowercase().as_str() {
                    "open" => IntentStatus::Open,
                    "blocked" => IntentStatus::Blocked,
                    "done" => IntentStatus::Done,
                    "superseded" => IntentStatus::Superseded,
                    _ => anyhow::bail!(
                        "unknown status: {s}. Expected one of: open, blocked, done, superseded."
                    ),
                };
            }
            intent.updated_at_ms = current_time_ms()?;
            let new_id = store_intent(&store, &intent)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "intent.update",
                        "updated": true,
                        "intent": intent_json(&intent, Some(new_id.to_hex())),
                    }))?
                );
            } else {
                println!("Updated intent: {}", intent.id);
            }
        }
        IntentCommand::Policy { command } => run_policy_command(command, json)?,
    }
    Ok(())
}

fn run_policy_command(command: IntentPolicyCommand, json: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    match command {
        IntentPolicyCommand::List { id } => {
            let (intent, object_id) = load_intent(&store, &id)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "intent.policy.list",
                        "intent_id": intent.id.to_string(),
                        "object_id": object_id.to_hex(),
                        "policy_refs": intent.policy_refs,
                    }))?
                );
            } else if intent.policy_refs.is_empty() {
                println!("No policy refs attached to intent {}", intent.id);
            } else {
                for policy_ref in &intent.policy_refs {
                    println!("{policy_ref}");
                }
            }
        }
        IntentPolicyCommand::Add {
            id,
            policy_ref,
            dry_run,
        } => {
            let policy_ref = normalized_policy_ref(&policy_ref)?;
            ensure_policy_ref_exists(&store, &policy_ref)?;
            let (mut intent, old_object_id) = load_intent(&store, &id)?;
            let changed = add_policy_ref(&mut intent.policy_refs, policy_ref.clone());
            let new_object_id = if !dry_run && changed {
                intent.updated_at_ms = current_time_ms()?;
                Some(store_intent(&store, &intent)?)
            } else {
                None
            };
            print_policy_update(
                &intent,
                &policy_ref,
                "add",
                changed,
                dry_run,
                Some(old_object_id.to_hex()),
                new_object_id.map(|id| id.to_hex()),
                json,
            )?;
        }
        IntentPolicyCommand::Remove {
            id,
            policy_ref,
            dry_run,
        } => {
            let policy_ref = normalized_policy_ref(&policy_ref)?;
            let (mut intent, old_object_id) = load_intent(&store, &id)?;
            let changed = remove_policy_ref(&mut intent.policy_refs, &policy_ref);
            let new_object_id = if !dry_run && changed {
                intent.updated_at_ms = current_time_ms()?;
                Some(store_intent(&store, &intent)?)
            } else {
                None
            };
            print_policy_update(
                &intent,
                &policy_ref,
                "remove",
                changed,
                dry_run,
                Some(old_object_id.to_hex()),
                new_object_id.map(|id| id.to_hex()),
                json,
            )?;
        }
    }

    Ok(())
}

fn load_intent(store: &ClawStore, id: &str) -> anyhow::Result<(Intent, claw_core::id::ObjectId)> {
    let obj_id = store.get_ref(&format!("intents/{id}"))?.ok_or_else(|| {
        anyhow::anyhow!(
            "intent not found: {id}. Run `claw intent list` to inspect available intents."
        )
    })?;
    let obj = store.load_object(&obj_id)?;
    match obj {
        Object::Intent(intent) => Ok((intent, obj_id)),
        _ => anyhow::bail!("intent ref points to a non-intent object: intents/{id}"),
    }
}

fn store_intent(store: &ClawStore, intent: &Intent) -> anyhow::Result<claw_core::id::ObjectId> {
    let id = store.store_object(&Object::Intent(intent.clone()))?;
    store.set_ref(&format!("intents/{}", intent.id), &id)?;
    Ok(id)
}

fn normalized_policy_ref(policy_ref: &str) -> anyhow::Result<String> {
    let value = policy_ref.trim();
    if value.is_empty() {
        anyhow::bail!("policy ref cannot be empty");
    }
    Ok(value.strip_prefix("policies/").unwrap_or(value).to_string())
}

fn ensure_policy_ref_exists(store: &ClawStore, policy_ref: &str) -> anyhow::Result<()> {
    if store.get_ref(policy_ref)?.is_some() {
        return Ok(());
    }
    let prefixed = format!("policies/{policy_ref}");
    if store.get_ref(&prefixed)?.is_some() {
        return Ok(());
    }
    anyhow::bail!(
        "policy ref not found: {policy_ref}. Run `claw policy show {policy_ref}` or `claw policy create --id {policy_ref}`."
    )
}

fn add_policy_ref(policy_refs: &mut Vec<String>, policy_ref: String) -> bool {
    if policy_refs.iter().any(|existing| existing == &policy_ref) {
        return false;
    }
    policy_refs.push(policy_ref);
    policy_refs.sort();
    true
}

fn remove_policy_ref(policy_refs: &mut Vec<String>, policy_ref: &str) -> bool {
    let original_len = policy_refs.len();
    policy_refs.retain(|existing| existing != policy_ref);
    original_len != policy_refs.len()
}

#[allow(clippy::too_many_arguments)]
fn print_policy_update(
    intent: &Intent,
    policy_ref: &str,
    operation: &str,
    changed: bool,
    dry_run: bool,
    old_object_id: Option<String>,
    new_object_id: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": format!("intent.policy.{operation}"),
                "operation": operation,
                "changed": changed,
                "dry_run": dry_run,
                "policy_ref": policy_ref,
                "old_object_id": old_object_id,
                "new_object_id": new_object_id,
                "intent": intent_json(intent, new_object_id),
            }))?
        );
    } else if dry_run {
        let verb = if changed {
            "would update"
        } else {
            "already unchanged"
        };
        println!("{verb}: intent {} policy ref {policy_ref}", intent.id);
    } else if changed {
        println!("Updated intent {} policy ref {policy_ref}", intent.id);
    } else {
        println!("No change: intent {} policy ref {policy_ref}", intent.id);
    }
    Ok(())
}

fn run_graph(store: &ClawStore, format: &str, json: bool) -> anyhow::Result<()> {
    let mut intents = Vec::new();
    for (_name, id) in store.list_refs("intents")? {
        if let Ok(Object::Intent(intent)) = store.load_object(&id) {
            intents.push((id, intent));
        }
    }

    let mut changes = Vec::new();
    for (_name, id) in store.list_refs("changes")? {
        if let Ok(Object::Change(change)) = store.load_object(&id) {
            changes.push((id, change));
        }
    }

    let (nodes, edges) = graph_nodes_edges(store, &intents, &changes);

    if json || format.eq_ignore_ascii_case("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "intent.graph",
                "intent_count": intents.len(),
                "change_count": changes.len(),
                "node_count": nodes.len(),
                "edge_count": edges.len(),
                "nodes": nodes,
                "edges": edges,
                "intents": intents.iter().map(|(object_id, intent)| serde_json::json!({
                    "object_id": object_id.to_hex(),
                    "id": intent.id.to_string(),
                    "title": intent.title,
                    "status": intent_status(intent.status),
                    "policies": intent.policy_refs,
                    "agents": intent.agents,
                    "acceptance_tests": intent.acceptance_tests,
                    "depends_on": intent.depends_on,
                    "supersedes": intent.supersedes,
                    "changes": intent.change_ids,
                })).collect::<Vec<_>>(),
                "changes": changes.iter().map(|(object_id, change)| serde_json::json!({
                    "object_id": object_id.to_hex(),
                    "id": change.id.to_string(),
                    "intent_id": change.intent_id.to_string(),
                    "status": format!("{:?}", change.status).to_ascii_lowercase(),
                    "head_revision": change.head_revision.map(|id| id.to_hex()),
                })).collect::<Vec<_>>(),
            }))?
        );
        return Ok(());
    }

    if format.eq_ignore_ascii_case("html") {
        println!(
            "{}",
            render_graph_html(&nodes, &edges, intents.len(), changes.len())?
        );
        return Ok(());
    }

    if !format.eq_ignore_ascii_case("mermaid") {
        anyhow::bail!("unknown graph format '{format}'; expected mermaid|json|html");
    }

    println!("flowchart LR");
    if intents.is_empty() {
        println!("  empty[\"No intents\"]");
        return Ok(());
    }

    for (_object_id, intent) in &intents {
        let node = graph_node_id("intent", &intent.id.to_string());
        println!(
            "  {node}[\"Intent: {}\\n{}\"]",
            escape_mermaid(&intent.title),
            intent_status(intent.status)
        );
        for policy in &intent.policy_refs {
            let policy_node = graph_node_id("policy", policy);
            println!("  {policy_node}[\"Policy: {}\"]", escape_mermaid(policy));
            println!("  {node} --> {policy_node}");
        }
        for agent in &intent.agents {
            let agent_node = graph_node_id("agent", agent);
            println!("  {agent_node}[\"Agent: {}\"]", escape_mermaid(agent));
            println!("  {agent_node} --> {node}");
        }
        if matches!(intent.status, IntentStatus::Blocked) {
            let blocker_node = graph_node_id("blocker", &intent.id.to_string());
            println!("  {blocker_node}[\"Blocker: blocked intent\"]");
            println!("  {blocker_node} --> {node}");
        }
        for test in &intent.acceptance_tests {
            let test_node = graph_node_id("acceptance", test);
            println!("  {test_node}[\"Acceptance: {}\"]", escape_mermaid(test));
            println!("  {node} --> {test_node}");
        }
        for dependency in &intent.depends_on {
            println!("  {} --> {node}", graph_node_id("intent", dependency));
        }
        for superseded in &intent.supersedes {
            println!(
                "  {} -. supersedes .-> {node}",
                graph_node_id("intent", superseded)
            );
        }
    }

    for (_object_id, change) in &changes {
        let change_node = graph_node_id("change", &change.id.to_string());
        let intent_node = graph_node_id("intent", &change.intent_id.to_string());
        println!(
            "  {change_node}[\"Change: {}\\n{:?}\"]",
            change.id, change.status
        );
        println!("  {intent_node} --> {change_node}");
        if let Some(revision) = change.head_revision {
            let revision_node = graph_node_id("revision", &revision.to_hex());
            println!(
                "  {revision_node}[\"Revision: {}\"]",
                &revision.to_hex()[..12]
            );
            println!("  {change_node} --> {revision_node}");
            if let Ok(Object::Revision(revision_object)) = store.load_object(&revision) {
                if let Ok((capsule_id, capsule)) =
                    load_default_capsule(store, &revision, &revision_object)
                {
                    let capsule_node = graph_node_id("capsule", &capsule_id.to_hex());
                    println!(
                        "  {capsule_node}[\"Capsule: {}\\nagent={}\"]",
                        &capsule_id.to_hex()[..12],
                        escape_mermaid(&capsule.public_fields.agent_id)
                    );
                    println!("  {revision_node} --> {capsule_node}");
                    for evidence in &capsule.public_fields.evidence {
                        let evidence_node = graph_node_id(
                            "evidence",
                            &format!("{}:{}", capsule_id.to_hex(), evidence.name),
                        );
                        println!(
                            "  {evidence_node}[\"Evidence: {}\\n{}\"]",
                            escape_mermaid(&evidence.name),
                            escape_mermaid(&evidence.status)
                        );
                        println!("  {capsule_node} --> {evidence_node}");
                    }
                }
            }
        }
    }

    Ok(())
}

fn graph_nodes_edges(
    store: &ClawStore,
    intents: &[(claw_core::id::ObjectId, Intent)],
    changes: &[(claw_core::id::ObjectId, claw_core::types::Change)],
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut seen_nodes = std::collections::BTreeSet::new();
    let mut seen_edges = std::collections::BTreeSet::new();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for (object_id, intent) in intents {
        let intent_node = format!("intent:{}", intent.id);
        add_graph_node(
            &mut nodes,
            &mut seen_nodes,
            serde_json::json!({
                "id": intent_node,
                "type": "intent",
                "label": intent.title,
                "object_id": object_id.to_hex(),
                "status": intent_status(intent.status),
            }),
        );
        if matches!(intent.status, IntentStatus::Blocked) {
            let blocker_node = format!("blocker:{}:status", intent.id);
            add_graph_node(
                &mut nodes,
                &mut seen_nodes,
                serde_json::json!({
                    "id": blocker_node,
                    "type": "blocker",
                    "label": "blocked intent",
                    "intent_id": intent.id.to_string(),
                }),
            );
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &format!("blocker:{}:status", intent.id),
                &format!("intent:{}", intent.id),
                "blocks",
            );
        }
        for policy in &intent.policy_refs {
            let policy_node = format!("policy:{policy}");
            add_graph_node(
                &mut nodes,
                &mut seen_nodes,
                serde_json::json!({
                    "id": policy_node,
                    "type": "policy",
                    "label": policy,
                }),
            );
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &intent_node,
                &policy_node,
                "requires",
            );
        }
        for agent in &intent.agents {
            let agent_node = format!("agent:{agent}");
            add_graph_node(
                &mut nodes,
                &mut seen_nodes,
                serde_json::json!({
                    "id": agent_node,
                    "type": "agent",
                    "label": agent,
                }),
            );
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &agent_node,
                &intent_node,
                "owns",
            );
        }
        for test in &intent.acceptance_tests {
            let spec_node = format!("acceptance:{test}");
            add_graph_node(
                &mut nodes,
                &mut seen_nodes,
                serde_json::json!({
                    "id": spec_node,
                    "type": "acceptance",
                    "label": test,
                }),
            );
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &intent_node,
                &spec_node,
                "accepts_with",
            );
        }
        for dependency in &intent.depends_on {
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &format!("intent:{dependency}"),
                &intent_node,
                "dependency",
            );
        }
        for superseded in &intent.supersedes {
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &format!("intent:{superseded}"),
                &intent_node,
                "superseded_by",
            );
        }
    }

    for (object_id, change) in changes {
        let change_node = format!("change:{}", change.id);
        add_graph_node(
            &mut nodes,
            &mut seen_nodes,
            serde_json::json!({
                "id": change_node,
                "type": "change",
                "label": change.id.to_string(),
                "object_id": object_id.to_hex(),
                "status": change_status(change.status),
                "intent_id": change.intent_id.to_string(),
            }),
        );
        add_graph_edge(
            &mut edges,
            &mut seen_edges,
            &format!("intent:{}", change.intent_id),
            &change_node,
            "contains_change",
        );
        if let Some(revision_id) = change.head_revision {
            let revision_node = format!("revision:{}", revision_id.to_hex());
            add_graph_node(
                &mut nodes,
                &mut seen_nodes,
                serde_json::json!({
                    "id": revision_node,
                    "type": "revision",
                    "label": &revision_id.to_hex()[..12],
                    "object_id": revision_id.to_hex(),
                }),
            );
            add_graph_edge(
                &mut edges,
                &mut seen_edges,
                &change_node,
                &revision_node,
                "head",
            );
            if let Ok(Object::Revision(revision)) = store.load_object(&revision_id) {
                if let Ok((capsule_id, capsule)) =
                    load_default_capsule(store, &revision_id, &revision)
                {
                    let capsule_node = format!("capsule:{}", capsule_id.to_hex());
                    add_graph_node(
                        &mut nodes,
                        &mut seen_nodes,
                        serde_json::json!({
                            "id": capsule_node,
                            "type": "capsule",
                            "label": &capsule_id.to_hex()[..12],
                            "object_id": capsule_id.to_hex(),
                            "agent_id": capsule.public_fields.agent_id,
                            "evidence_count": capsule.public_fields.evidence.len(),
                            "signature_count": capsule.signatures.len(),
                        }),
                    );
                    add_graph_edge(
                        &mut edges,
                        &mut seen_edges,
                        &revision_node,
                        &capsule_node,
                        "has_capsule",
                    );
                    let agent_node = format!("agent:{}", capsule.public_fields.agent_id);
                    add_graph_node(
                        &mut nodes,
                        &mut seen_nodes,
                        serde_json::json!({
                            "id": agent_node,
                            "type": "agent",
                            "label": capsule.public_fields.agent_id,
                        }),
                    );
                    add_graph_edge(
                        &mut edges,
                        &mut seen_edges,
                        &agent_node,
                        &capsule_node,
                        "signed",
                    );
                    for evidence in &capsule.public_fields.evidence {
                        let evidence_node =
                            format!("evidence:{}:{}", capsule_id.to_hex(), evidence.name);
                        add_graph_node(
                            &mut nodes,
                            &mut seen_nodes,
                            serde_json::json!({
                                "id": evidence_node,
                                "type": "evidence",
                                "label": evidence.name,
                                "status": evidence.status,
                                "summary": evidence.summary,
                                "runner_identity": evidence.runner_identity,
                            }),
                        );
                        add_graph_edge(
                            &mut edges,
                            &mut seen_edges,
                            &capsule_node,
                            &format!("evidence:{}:{}", capsule_id.to_hex(), evidence.name),
                            "claims",
                        );
                    }
                }
            }
        }
    }

    (nodes, edges)
}

fn add_graph_node(
    nodes: &mut Vec<serde_json::Value>,
    seen: &mut std::collections::BTreeSet<String>,
    node: serde_json::Value,
) {
    let Some(id) = node["id"].as_str() else {
        return;
    };
    if seen.insert(id.to_string()) {
        nodes.push(node);
    }
}

fn add_graph_edge(
    edges: &mut Vec<serde_json::Value>,
    seen: &mut std::collections::BTreeSet<String>,
    from: &str,
    to: &str,
    relation: &str,
) {
    let key = format!("{from}\0{to}\0{relation}");
    if seen.insert(key) {
        edges.push(serde_json::json!({
            "from": from,
            "to": to,
            "relation": relation,
        }));
    }
}

fn render_graph_html(
    nodes: &[serde_json::Value],
    edges: &[serde_json::Value],
    intent_count: usize,
    change_count: usize,
) -> anyhow::Result<String> {
    let nodes_json = serde_json::to_string(nodes)?.replace("</", "<\\/");
    let edges_json = serde_json::to_string(edges)?.replace("</", "<\\/");
    let template = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Claw Intent Graph</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f7f8fb;
      --panel: #ffffff;
      --ink: #1f2937;
      --muted: #667085;
      --line: #d5d9e2;
      --intent: #2563eb;
      --change: #059669;
      --revision: #7c3aed;
      --capsule: #c2410c;
      --evidence: #0f766e;
      --policy: #be123c;
      --agent: #4d7c0f;
      --blocker: #b91c1c;
      --acceptance: #0369a1;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      background: var(--bg);
      color: var(--ink);
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 24px;
      padding: 18px 24px;
      border-bottom: 1px solid var(--line);
      background: var(--panel);
    }
    h1 {
      margin: 0;
      font-size: 18px;
      line-height: 1.2;
      letter-spacing: 0;
    }
    .summary {
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      color: var(--muted);
      font-size: 13px;
    }
    .layout {
      display: grid;
      grid-template-columns: minmax(0, 1fr) 320px;
      min-height: calc(100vh - 64px);
    }
    main {
      min-width: 0;
      overflow: auto;
      padding: 20px;
    }
    aside {
      border-left: 1px solid var(--line);
      background: var(--panel);
      padding: 18px;
      overflow: auto;
    }
    .toolbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 10px;
      margin-bottom: 14px;
    }
    input, select {
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--panel);
      color: var(--ink);
      font: inherit;
      font-size: 13px;
      min-height: 34px;
      padding: 6px 10px;
    }
    #search { min-width: min(360px, 100%); }
    svg {
      display: block;
      min-width: 960px;
      width: 100%;
      height: 680px;
      border: 1px solid var(--line);
      background: var(--panel);
    }
    .edge {
      stroke: #98a2b3;
      stroke-width: 1.4;
      marker-end: url(#arrow);
    }
    .edge-label {
      fill: var(--muted);
      font-size: 11px;
      pointer-events: none;
    }
    .node text {
      fill: #ffffff;
      font-size: 12px;
      pointer-events: none;
    }
    .node rect {
      stroke: rgba(0, 0, 0, .14);
      stroke-width: 1;
      rx: 7;
    }
    .node.dim, .edge.dim, .edge-label.dim { opacity: .16; }
    .node:hover rect { stroke-width: 2; }
    .details-title {
      margin: 0 0 10px;
      font-size: 16px;
    }
    .kv {
      display: grid;
      grid-template-columns: 90px minmax(0, 1fr);
      gap: 8px;
      border-top: 1px solid var(--line);
      padding: 10px 0;
      font-size: 13px;
    }
    .kv dt { color: var(--muted); }
    .kv dd { margin: 0; overflow-wrap: anywhere; }
    pre {
      white-space: pre-wrap;
      overflow-wrap: anywhere;
      padding: 10px;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: #f3f4f6;
      font-size: 12px;
    }
    @media (max-width: 880px) {
      header { align-items: flex-start; flex-direction: column; }
      .layout { grid-template-columns: 1fr; }
      aside { border-left: 0; border-top: 1px solid var(--line); }
      svg { min-width: 720px; height: 560px; }
    }
  </style>
</head>
<body>
  <header>
    <h1>Claw Intent Graph</h1>
    <div class="summary">
      <span>__INTENT_COUNT__ intents</span>
      <span>__CHANGE_COUNT__ changes</span>
      <span><span id="nodeCount">0</span> nodes</span>
      <span><span id="edgeCount">0</span> edges</span>
    </div>
  </header>
  <div class="layout">
    <main>
      <div class="toolbar">
        <input id="search" type="search" placeholder="Filter nodes">
        <select id="typeFilter" aria-label="Node type filter">
          <option value="">All node types</option>
        </select>
      </div>
      <svg id="graph" role="img" aria-label="Intent graph"></svg>
    </main>
    <aside id="details">
      <h2 class="details-title">Select a node</h2>
      <p class="summary">Inspect goals, changes, revisions, evidence, policies, agents, and blockers from the graph.</p>
    </aside>
  </div>
  <script>
    const nodes = __NODES__;
    const edges = __EDGES__;
    const colors = {
      intent: "#2563eb",
      change: "#059669",
      revision: "#7c3aed",
      capsule: "#c2410c",
      evidence: "#0f766e",
      policy: "#be123c",
      agent: "#4d7c0f",
      blocker: "#b91c1c",
      acceptance: "#0369a1"
    };
    const order = ["intent", "policy", "agent", "acceptance", "blocker", "change", "revision", "capsule", "evidence"];
    const svg = document.getElementById("graph");
    const details = document.getElementById("details");
    const search = document.getElementById("search");
    const typeFilter = document.getElementById("typeFilter");
    document.getElementById("nodeCount").textContent = nodes.length;
    document.getElementById("edgeCount").textContent = edges.length;

    [...new Set(nodes.map(node => node.type).filter(Boolean))].sort().forEach(type => {
      const option = document.createElement("option");
      option.value = type;
      option.textContent = type;
      typeFilter.appendChild(option);
    });

    function labelFor(node) {
      return String(node.label || node.id || "");
    }

    function layoutNodes() {
      const buckets = new Map();
      nodes.forEach(node => {
        const type = node.type || "other";
        if (!buckets.has(type)) buckets.set(type, []);
        buckets.get(type).push(node);
      });
      let x = 70;
      const positions = new Map();
      order.concat([...buckets.keys()].filter(type => !order.includes(type))).forEach(type => {
        const bucket = buckets.get(type) || [];
        bucket.forEach((node, index) => {
          positions.set(node.id, { x, y: 72 + index * 84 });
        });
        if (bucket.length) x += 190;
      });
      return positions;
    }

    function visibleNodeIds() {
      const q = search.value.trim().toLowerCase();
      const selectedType = typeFilter.value;
      return new Set(nodes.filter(node => {
        const text = JSON.stringify(node).toLowerCase();
        return (!selectedType || node.type === selectedType) && (!q || text.includes(q));
      }).map(node => node.id));
    }

    function showDetails(node) {
      details.innerHTML = "";
      const title = document.createElement("h2");
      title.className = "details-title";
      title.textContent = labelFor(node);
      details.appendChild(title);
      const dl = document.createElement("dl");
      for (const [key, value] of Object.entries(node)) {
        const row = document.createElement("div");
        row.className = "kv";
        const dt = document.createElement("dt");
        const dd = document.createElement("dd");
        dt.textContent = key;
        dd.textContent = Array.isArray(value) || typeof value === "object" ? JSON.stringify(value) : String(value);
        row.append(dt, dd);
        dl.appendChild(row);
      }
      details.appendChild(dl);
      const related = edges.filter(edge => edge.from === node.id || edge.to === node.id);
      const pre = document.createElement("pre");
      pre.textContent = JSON.stringify({ related_edges: related }, null, 2);
      details.appendChild(pre);
    }

    function render() {
      const positions = layoutNodes();
      const visible = visibleNodeIds();
      const maxY = Math.max(560, ...[...positions.values()].map(pos => pos.y + 80));
      const maxX = Math.max(940, ...[...positions.values()].map(pos => pos.x + 170));
      svg.setAttribute("viewBox", `0 0 ${maxX} ${maxY}`);
      svg.innerHTML = `<defs><marker id="arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="#98a2b3"></path></marker></defs>`;
      edges.forEach(edge => {
        const from = positions.get(edge.from);
        const to = positions.get(edge.to);
        if (!from || !to) return;
        const active = visible.has(edge.from) && visible.has(edge.to);
        const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
        line.setAttribute("x1", from.x + 132);
        line.setAttribute("y1", from.y + 24);
        line.setAttribute("x2", to.x);
        line.setAttribute("y2", to.y + 24);
        line.setAttribute("class", active ? "edge" : "edge dim");
        svg.appendChild(line);
        const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
        label.setAttribute("x", (from.x + to.x + 132) / 2);
        label.setAttribute("y", (from.y + to.y) / 2 + 16);
        label.setAttribute("class", active ? "edge-label" : "edge-label dim");
        label.textContent = edge.relation || "";
        svg.appendChild(label);
      });
      nodes.forEach(node => {
        const pos = positions.get(node.id);
        if (!pos) return;
        const group = document.createElementNS("http://www.w3.org/2000/svg", "g");
        group.setAttribute("class", visible.has(node.id) ? "node" : "node dim");
        group.setAttribute("tabindex", "0");
        group.setAttribute("role", "button");
        const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
        rect.setAttribute("x", pos.x);
        rect.setAttribute("y", pos.y);
        rect.setAttribute("width", 132);
        rect.setAttribute("height", 48);
        rect.setAttribute("fill", colors[node.type] || "#475467");
        const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
        text.setAttribute("x", pos.x + 10);
        text.setAttribute("y", pos.y + 21);
        const typeLine = document.createElementNS("http://www.w3.org/2000/svg", "tspan");
        typeLine.textContent = node.type || "node";
        const labelLine = document.createElementNS("http://www.w3.org/2000/svg", "tspan");
        labelLine.setAttribute("x", pos.x + 10);
        labelLine.setAttribute("dy", 16);
        const label = labelFor(node);
        labelLine.textContent = label.length > 18 ? `${label.slice(0, 17)}...` : label;
        text.append(typeLine, labelLine);
        group.append(rect, text);
        group.addEventListener("click", () => showDetails(node));
        group.addEventListener("keydown", event => {
          if (event.key === "Enter" || event.key === " ") showDetails(node);
        });
        svg.appendChild(group);
      });
    }
    search.addEventListener("input", render);
    typeFilter.addEventListener("change", render);
    render();
  </script>
</body>
</html>
"##;

    Ok(template
        .replace("__NODES__", &nodes_json)
        .replace("__EDGES__", &edges_json)
        .replace("__INTENT_COUNT__", &intent_count.to_string())
        .replace("__CHANGE_COUNT__", &change_count.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcceptanceOutcome {
    Passed,
    Failed,
    TimedOut,
}

impl AcceptanceOutcome {
    fn status(self) -> &'static str {
        match self {
            Self::Passed => "pass",
            Self::Failed => "fail",
            Self::TimedOut => "timeout",
        }
    }
}

struct AcceptanceResult {
    name: String,
    command: String,
    outcome: AcceptanceOutcome,
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout: String,
    stderr: String,
}

fn run_acceptance(
    root: &std::path::Path,
    intent: &Intent,
    object_id: claw_core::id::ObjectId,
    timeout_ms: u64,
    keep_going: bool,
    json: bool,
) -> anyhow::Result<()> {
    if intent.acceptance_tests.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "action": "intent.run_acceptance",
                    "intent_id": intent.id.to_string(),
                    "intent_object": object_id.to_hex(),
                    "passed": true,
                    "count": 0,
                    "results": [],
                    "evidence": [],
                }))?
            );
        } else {
            println!("No acceptance tests linked to intent {}", intent.id);
        }
        return Ok(());
    }

    let mut results = Vec::new();
    for (index, command) in intent.acceptance_tests.iter().enumerate() {
        let result = run_acceptance_command(
            root,
            format!("acceptance/{}", index + 1),
            command,
            timeout_ms,
        )?;
        let failed = result.outcome != AcceptanceOutcome::Passed;
        results.push(result);
        if failed && !keep_going {
            break;
        }
    }

    let passed = results
        .iter()
        .all(|result| result.outcome == AcceptanceOutcome::Passed)
        && results.len() == intent.acceptance_tests.len();
    let evidence = results
        .iter()
        .map(acceptance_evidence_json)
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "intent.run_acceptance",
                "intent_id": intent.id.to_string(),
                "intent_object": object_id.to_hex(),
                "passed": passed,
                "count": results.len(),
                "expected_count": intent.acceptance_tests.len(),
                "timeout_ms": timeout_ms,
                "keep_going": keep_going,
                "results": results.iter().map(acceptance_result_json).collect::<Vec<_>>(),
                "evidence": evidence,
            }))?
        );
    } else {
        for result in &results {
            println!(
                "{} {} duration_ms={} command={}",
                result.name,
                result.outcome.status(),
                result.duration_ms,
                result.command
            );
            if !result.stdout.trim().is_empty() {
                println!("  stdout: {}", result.stdout.trim());
            }
            if !result.stderr.trim().is_empty() {
                println!("  stderr: {}", result.stderr.trim());
            }
        }
        if passed {
            println!("Acceptance passed for intent {}", intent.id);
        } else {
            println!("Acceptance failed for intent {}", intent.id);
        }
    }

    if passed {
        Ok(())
    } else {
        anyhow::bail!("acceptance failed for intent {}", intent.id)
    }
}

fn run_acceptance_command(
    root: &std::path::Path,
    name: String,
    command: &str,
    timeout_ms: u64,
) -> anyhow::Result<AcceptanceResult> {
    let start = Instant::now();
    let mut child = shell_command(command)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            anyhow::anyhow!("failed to run acceptance command '{}': {}", command, err)
        })?;

    let timeout = Duration::from_millis(timeout_ms);
    let (outcome, exit_code, output) = loop {
        if let Some(status) = child.try_wait()? {
            let output = child.wait_with_output()?;
            let outcome = if status.success() {
                AcceptanceOutcome::Passed
            } else {
                AcceptanceOutcome::Failed
            };
            break (outcome, status.code(), output);
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            break (AcceptanceOutcome::TimedOut, None, output);
        }

        std::thread::sleep(Duration::from_millis(25));
    };

    let duration_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    Ok(AcceptanceResult {
        name,
        command: command.to_string(),
        outcome,
        exit_code,
        duration_ms,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn acceptance_result_json(result: &AcceptanceResult) -> serde_json::Value {
    serde_json::json!({
        "name": result.name,
        "command": result.command,
        "status": result.outcome.status(),
        "exit_code": result.exit_code,
        "duration_ms": result.duration_ms,
        "stdout": result.stdout,
        "stderr": result.stderr,
    })
}

fn acceptance_evidence_json(result: &AcceptanceResult) -> serde_json::Value {
    serde_json::json!({
        "name": result.name,
        "status": result.outcome.status(),
        "duration_ms": result.duration_ms,
        "command": result.command,
        "exit_code": result.exit_code,
        "summary": format!("{} {}", result.name, result.outcome.status()),
    })
}

fn graph_node_id(prefix: &str, value: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + value.len() + 1);
    out.push_str(prefix);
    out.push('_');
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

fn escape_mermaid(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn current_time_ms() -> anyhow::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64)
}

fn intent_json(intent: &claw_core::types::Intent, object_id: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "id": intent.id.to_string(),
        "object_id": object_id,
        "title": intent.title,
        "goal": intent.goal,
        "status": intent_status(intent.status),
        "acceptance_tests": intent.acceptance_tests,
        "constraints": intent.constraints,
        "links": intent.links,
        "agents": intent.agents,
        "depends_on": intent.depends_on,
        "supersedes": intent.supersedes,
        "policy_refs": intent.policy_refs,
        "change_ids": intent.change_ids,
        "created_at_ms": intent.created_at_ms,
        "updated_at_ms": intent.updated_at_ms,
    })
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
