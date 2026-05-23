# Claw Agent SDK for Python

This package provides a small CLI-backed client for Python agents.

```python
from claw_agent_sdk import (
    AgentAuditRequest,
    AgentBulkRequest,
    ClawClient,
    EvidenceQuery,
    PolicySimulateRequest,
    ReviewRequest,
    StoryExportRequest,
    TrustReceiptRequest,
)

claw = ClawClient(cwd="/path/to/repo")
intent = claw.create_intent(
    "Fix cache invalidation",
    "Make cache behavior explicit",
    acceptance_tests=["cargo test -p claw-vcs --bin claw"],
)
graph = claw.intent_graph()
health = claw.doctor_deep()
repair_plan = claw.repair_plan()
quarantined = claw.agent_audit(AgentAuditRequest(
    status="quarantined",
    action_required=True,
))
fleet_preview = claw.agent_bulk(AgentBulkRequest(
    file="agent-fleet.json",
    dry_run=True,
))
story = claw.story_export(StoryExportRequest(intent=str(intent.get("id", ""))))
evidence = claw.query_evidence(EvidenceQuery(
    query="(test=pass OR lint=pass) AND trust>=0.8",
    limit=10,
))
simulation = claw.policy_simulate(PolicySimulateRequest(
    policy_id="ci-required",
    revision="heads/main",
))
review = claw.review(ReviewRequest(intent=intent.get("id")))
trust = claw.trust_receipt(TrustReceiptRequest(revision="heads/main"))
```

The SDK provides typed request objects for intents, changes, acceptance tests,
intent graphs, evidence queries, policy simulation, review reports, capsule
inspection, trust receipts, shipping, provenance replay, SLSA/in-toto
attestation attachment, deep health checks, repair planning, filtered agent
audits, dry-runnable agent bulk plans, story export, timeline allowance
debugging, and patch workbench receipts.

MCP-compatible hosts can launch the same Claw tool surface with:

```bash
claw mcp serve
```

Use `claw mcp serve --allow-write` only for trusted agents that may mutate the
repository.
