# Claw Agent SDK for Rust

This crate provides small, typed helpers for agents that drive the `claw` CLI.
It wraps command execution, JSON parsing, and MCP request shapes without hiding
the underlying Claw command contract.

```rust
use claw_vcs_agent_sdk::{
    AgentAuditRequest, AgentBulkRequest, ClawClient, EvidenceQuery, PolicySimulateRequest,
    ReviewRequest, TrustReceiptRequest,
};

let client = ClawClient::default().with_cwd("/path/to/repo");
let status = client.status()?;
let intent = client.create_intent("Fix cache", "Make cache invalidation explicit")?;
let graph = client.intent_graph()?;
let health = client.doctor_deep()?;
let repair_plan = client.repair_plan()?;
let quarantined = client.agent_audit(AgentAuditRequest {
    status: Some("quarantined".into()),
    action_required: true,
    ..AgentAuditRequest::default()
})?;
let fleet_preview = client.agent_bulk(AgentBulkRequest {
    file: "agent-fleet.json".into(),
    dry_run: true,
})?;
let story = client.story_export(claw_vcs_agent_sdk::StoryExportRequest {
    intent: intent["id"].as_str().unwrap_or_default().into(),
})?;
let evidence = client.query_evidence_with(EvidenceQuery {
    query: "(test=pass OR lint=pass) AND trust>=0.8".into(),
    limit: Some(10),
    ..EvidenceQuery::default()
})?;
let simulation = client.policy_simulate(PolicySimulateRequest {
    policy_id: Some("ci-required".into()),
    revision: "heads/main".into(),
    ..PolicySimulateRequest::default()
})?;
let review = client.review(ReviewRequest {
    intent: intent["id"].as_str().map(str::to_string),
    ..ReviewRequest::default()
})?;
let trust = client.trust_receipt(TrustReceiptRequest {
    revision: "heads/main".into(),
    ..TrustReceiptRequest::default()
})?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The SDK exposes typed request objects for the common agent loop: intents,
intent graphs, changes, acceptance tests, evidence queries, policy simulation,
review reports, capsule inspection, trust receipts, shipping, provenance replay,
SLSA/in-toto attestation attachment, deep health checks, repair planning,
filtered agent audits, and dry-runnable agent bulk plans. It also exposes story
export, timeline allowance debugging, and patch workbench receipts for
agent-side explanation and review flows.
The CLI remains the source of truth for repository behavior. Pin the `claw`
binary version used by production agents.
