# Agent docs

Claw treats agents as first-class producers of changes. Agents should create or
use intents, record changes, attach evidence, and sign capsules.

## Pages

- [Agent registration](agent-registration.md)
- [Agent change workflow](change-workflow.md)
- [Capsule evidence guide](capsule-evidence.md)
- [Evidence schema](evidence-schema.md)
- [Evidence freshness](evidence-freshness.md)
- [Key rotation and revocation](key-rotation-and-revocation.md)
- [Integration guide](integration-guide.md)

## SDKs and MCP

Agent integrations can use the CLI directly, the Rust crate
`claw-vcs-agent-sdk`, the lightweight TypeScript and Python SDKs under `sdk/`,
or the local MCP server:

```bash
claw mcp serve
claw mcp serve --allow-write
```

The default MCP server exposes read-only repository tools. `--allow-write`
adds intent and change creation tools for trusted agent runtimes.

## Agent contract

An agent should:

- run under a registered identity
- use a stable agent ID and version string
- attach repeatable evidence names
- avoid writing secrets to public capsule fields
- keep private capsule fields encrypted when policy requires it
- stop when policy denies a ship or integration

## Minimal CLI integration

```bash
claw agent register --name docs-agent --version "2026-05-11"
claw intent create --title "Update docs" --goal "Clarify agent workflow"
claw change create --intent <intent-id>
claw snapshot --change <change-id> -m "Clarify agent workflow"
claw ship --intent <intent-id> --revision-ref heads/main --agent docs-agent --evidence test=pass
```

Use `claw agent status <name>` before signing if the runner may have lost its
local key. A usable local signer reports `Key: ... (verified)`; `claw agent
list` shows the compact form `key:verified`.

## Current limits

- `claw agent keygen --name <agent>` provisions a local signing key without
  changing repository trust.
- `claw agent register --public-key <hex>` and
  `claw agent rotate --public-key <hex>` support externally managed Ed25519
  keys.
- `claw agent rotate`, `claw agent quarantine`, `claw agent unquarantine`, and
  `claw agent revoke` update repository registrations for future policy
  decisions.
- `claw agent audit` scans fleet state across all `agents/*` refs and returns
  triage counts plus per-agent recommended actions.
- Policy enforcement depends on policies referenced by intents. Creating a
  policy object does not attach it to every intent.
