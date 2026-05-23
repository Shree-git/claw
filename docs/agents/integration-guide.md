# Agent Integration Guide

This guide describes the current v0.1 CLI path for agent-produced work.

Agents can integrate through four supported surfaces:

- CLI commands for shell-first automation.
- The Rust SDK crate `claw-vcs-agent-sdk`.
- The TypeScript SDK in `sdk/typescript`.
- The Python SDK in `sdk/python`.
- The MCP stdio server, `claw mcp serve`, for MCP-compatible hosts.

## Register

```bash
claw agent register --name demo-agent --version "2026-05-11"
claw agent status demo-agent
```

The CLI creates a local Ed25519 key under the user's Claw home and stores public registration metadata in the repository.

## Work

```bash
claw intent create --title "Task" --goal "Outcome"
claw change create --intent <intent-id>
# edit files
claw snapshot --change <change-id> -m "Implement task"
```

## Submit Evidence

```bash
claw ship \
  --intent <intent-id> \
  --revision-ref heads/main \
  --agent demo-agent \
  --evidence test=pass:1200 \
  --evidence lint=pass:300
```

Evidence is stored in the capsule public fields. Private capsule metadata is reserved for encrypted fields used by policy-sensitive workflows.

## Daemon API

Agents can use the daemon for sync, events, intents, changes, capsules, and workstreams. Use `claw daemon --help` and require auth/TLS for non-local production binds.

The stable v0.1 integration path is still the CLI. Use the daemon when the agent
needs a long-running service boundary, remote sync, or event notifications. A
minimal event-driven integration looks like this:

1. Start the daemon with an explicit profile and security boundary.

```bash
claw --profile prod daemon \
  --listen 127.0.0.1:9718 \
  --auth-profile demo-agent \
  --tls-cert ~/.config/claw/tls/server.crt \
  --tls-key ~/.config/claw/tls/server.key
```

2. Register the daemon remote and check compatibility before sending work.

```bash
claw remote add origin https://127.0.0.1:9718 --kind grpc --token-profile demo-agent
claw sync --tls-ca-cert ~/.config/claw/tls/ca.crt \
  push --remote origin --ref-name heads/main --dry-run
```

The client and daemon negotiate the `claw-sync/1` protocol and capabilities
such as `partial-clone`, `event-bus`, and `request-limits`. Treat a protocol
mismatch as a deployment error unless an operator explicitly allows recovery
with the documented compatibility flags. `claw sync push --dry-run` still
connects and advertises refs, but it skips object upload and ref mutation.

3. Subscribe to repository events, then react to ref changes.

```text
EventStreamService.Subscribe({
  event_types: ["ref_created", "ref_updated"],
  ref_prefix: "heads/"
})
```

Events are hints, not trust decisions. After an event arrives, reload the
referenced object or ref through the normal CLI/daemon path and verify policy
state before acting. The current gRPC service contract is defined in
`proto/claw/event.proto`; there is no stable `claw events` CLI in v0.1.x.

4. Submit work through the normal object workflow.

```bash
claw intent create --title "Task" --goal "Outcome" --json
claw change create --intent <intent-id> --json
claw snapshot --change <change-id> -m "Implement task" --json
claw ship --intent <intent-id> --revision-ref heads/main --agent demo-agent \
  --evidence test=pass:1200
```

5. Sync only the refs and objects the agent is authorized to read or update.

```bash
claw sync --tls-ca-cert ~/.config/claw/tls/ca.crt \
  push --remote origin --ref-name heads/main --dry-run
claw sync --tls-ca-cert ~/.config/claw/tls/ca.crt \
  push --remote origin --ref-name heads/main
```

Agents should request the narrowest daemon grants they need: read refs, fetch
objects, push objects, update refs, create intents, create changes, ship,
integrate, admin, and read private capsule fields are separate scopes. Private
capsule fields require both recipient envelope access and daemon authorization.

For HTTP health and metrics integration, use the committed OpenAPI artifact at
`docs/reference/daemon-http-openapi-v1.json`. For gRPC service contracts, treat
the generated protocol code and compatibility tests as the source of truth for
the current release line.

## MCP Server

Use MCP when the host already supports tool discovery and JSON-RPC over stdio:

```bash
claw mcp serve
```

The default MCP server is read-only. It exposes tools for status, intent lists,
evidence queries, review reports, agent audits, and trust receipts. Trusted
agents that need to create intents or changes can be launched with:

```bash
claw mcp serve --allow-write
```

See `docs/reference/mcp-server.md` for the current method and tool contract.

## Rotate or Revoke

```bash
claw agent rotate --name demo-agent --version "2026-05-12"
claw agent revoke --name demo-agent --reason "runner compromise"
```

Rotation replaces the trusted public key and local signing key for that agent ID.
Revocation marks the registration as untrusted for future signing and integration
decisions. In both cases, audit capsules signed by the previous key before
trusting affected revisions.
