# Claw MCP Server

`claw mcp serve` runs a Model Context Protocol server over newline-delimited
JSON-RPC on stdio. It is intended for local agent hosts that need Claw-native
tools without implementing the CLI command matrix themselves.

```bash
claw mcp serve
claw mcp serve --allow-write
```

The default server is read-only. Mutating tools are hidden unless
`--allow-write` is set.

## Methods

- `initialize`: returns Claw server metadata and tool capability support.
- `tools/list`: returns available Claw tools.
- `tools/call`: invokes one Claw tool and returns both text and structured JSON.
- `ping`: returns an empty success result.

## Tools

Read-only tools:

- `claw_status`
- `claw_intent_list`
- `claw_intent_graph`
- `claw_doctor`
- `claw_repair_plan`
- `claw_story_export`
- `claw_timeline_allowed`
- `claw_patch_workbench`
- `claw_evidence_query`
- `claw_capsule_inspect`
- `claw_review`
- `claw_agent_audit`
- `claw_trust_receipt`
- `claw_policy_simulate`

Write-enabled tools, available only with `--allow-write`:

- `claw_intent_create`
- `claw_run_acceptance`
- `claw_change_create`
- `claw_provenance_replay`
- `claw_attach_attestation` (supports subject, digest, SLSA builder, and build-type constraints)
- `claw_agent_bulk` (dry-run or apply ordered agent lifecycle plans)

Each tool delegates to the matching `claw --json` command. The CLI JSON schema
remains the source of truth for structured results.

`claw_trust_receipt` accepts the same evaluator context as the CLI trust
receipt command: `policies`, `signer_agents`, `signer_keys`, `paths`,
and `trust_score`.

`claw_doctor` accepts optional `deep` and `strict` booleans. `claw_repair_plan`
is always read-only and returns the same JSON as `claw repair --json plan`.
`claw_agent_audit` accepts optional `status`, `risk`, and `action_required`
filters. `claw_agent_bulk` accepts `file` and optional `dry_run`; it is hidden
unless write tools are enabled because non-dry-run plans mutate agent refs and
local keys.
`claw_story_export`, `claw_timeline_allowed`, and `claw_patch_workbench` expose
the same structured receipts as their CLI equivalents for narrative export,
policy allowance debugging, and patch algebra review.

## Security

Run MCP servers with the same care as any local repository automation. Read-only
mode still exposes repository metadata to the connected host. Write mode can
create repository objects and run repository-local commands, so it should be
limited to trusted agent runtimes.
