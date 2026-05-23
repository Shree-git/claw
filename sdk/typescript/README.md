# Claw Agent SDK for TypeScript

This package provides lightweight process helpers for agents that use the
`claw` CLI as their repository boundary.

```ts
import { ClawClient } from "@claw-vcs/agent-sdk";

const claw = new ClawClient({ cwd: "/path/to/repo" });
const intent = await claw.createIntent({
  title: "Fix cache invalidation",
  goal: "Make cache behavior explicit",
  acceptanceTests: ["cargo test -p claw-vcs --bin claw"]
});
const graph = await claw.intentGraph();
const health = await claw.doctorDeep();
const repairPlan = await claw.repairPlan();
const quarantined = await claw.agentAudit({
  status: "quarantined",
  actionRequired: true
});
const fleetPreview = await claw.agentBulk({
  file: "agent-fleet.json",
  dryRun: true
});
const story = await claw.storyExport({ intent: String((intent as any).id ?? "") });
const evidence = await claw.queryEvidence({
  query: "(test=pass OR lint=pass) AND trust>=0.8",
  limit: 10
});
const simulation = await claw.policySimulate({
  policyId: "ci-required",
  revision: "heads/main"
});
const review = await claw.review({ intent: String((intent as any).id ?? "") });
const trust = await claw.trustReceipt({ revision: "heads/main" });
```

The SDK provides typed request objects for intents, changes, acceptance tests,
intent graphs, evidence queries, policy simulation, review reports, capsule
inspection, trust receipts, shipping, provenance replay, SLSA/in-toto
attestation attachment, deep health checks, repair planning, filtered agent
audits, dry-runnable agent bulk plans, story export, timeline allowance
debugging, and patch workbench receipts.

For MCP-compatible hosts, launch:

```bash
claw mcp serve
```

Use `claw mcp serve --allow-write` only for trusted agents that may mutate the
repository.

Run the SDK typecheck from this package directory with:

```bash
npm install
npm run typecheck
```
