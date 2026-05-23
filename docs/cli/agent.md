# `claw agent`

Manage local agent registration records and signing keys.

```bash
claw agent keygen --name ci-agent
claw agent register --name ci-agent
claw agent register --name hosted-agent --public-key <hex-ed25519-public-key>
claw agent rotate --name ci-agent --version "2026-05-11"
claw agent rotate --name hosted-agent --public-key <replacement-public-key>
claw agent quarantine --name ci-agent --reason "runner drift"
claw agent unquarantine --name ci-agent
claw agent revoke --name ci-agent --reason "runner compromise"
claw agent bulk --file agent-fleet.json --dry-run --json
claw agent audit --json
claw agent audit --status revoked --json
claw agent audit --risk warning --action-required --json
claw agent list --json
claw agent status ci-agent --json
```

Agent private keys are stored outside the repository under the user Claw home.

`agent keygen` creates a local signing key without registering repository trust.
Use it when a runner needs to provision its key before a maintainer registers
the public key.

`agent register --public-key` records an externally managed Ed25519 public key
without creating a local signing key. Use plain `register` for local agents that
should sign from the current machine.

`agent rotate` replaces the trusted public key and local signing key for an existing agent. Use `--public-key` for externally managed replacement keys. Use `--dry-run` to preview the operation without updating the repository or key store.

`agent quarantine` temporarily removes an agent from signing and integration trust without treating old signatures as permanently revoked. Quarantined agents are omitted from the integration trust registry, and `claw ship --agent <name>` refuses to sign as that agent. Use `agent unquarantine` to restore trust after investigation, or `agent rotate` to replace the key and clear quarantine.

`agent revoke` marks the registration as revoked for future signing and integration decisions. Revoked agents are omitted from the integration trust registry, and `claw ship --agent <name>` refuses to sign as a revoked agent. Old signatures remain useful for attribution.

`agent bulk --file <json>` applies a fleet lifecycle plan in order. The JSON
file contains an `agents` array; each entry has an `action` of `register`,
`rotate`, `revoke`, `quarantine`, or `unquarantine`, plus the same fields as the
single-agent command (`name`, optional `version`, optional `public_key`, and
optional `reason` where applicable). Use `--dry-run --json` before applying a
large plan; the receipt reports `planned_count`, `changed_count`, and one result
per operation without updating refs or local keys.

`agent audit` scans all `agents/*` refs and reports active, quarantined,
revoked, legacy, malformed, missing-key, mismatched-key, and
private-key-material findings. `--json` emits `schema_version: 1`, `action:
agent.audit`, lifecycle counts, local-key counts, fleet triage counts
(`healthy`, `action_required`, `critical`, `warning`, `info`), an `agents`
inventory with one row per agent ref, and a `findings` array for fleet
automation across many agent keys. Each inventory row includes the ref name,
object id, record state, lifecycle status, local-key state, `risk_level`,
`action_required`, `recommended_action`, public-key prefix, timestamps, and
quarantine or revocation metadata.

Use `agent audit --status <active|quarantined|revoked|legacy|malformed>`,
`--risk <none|info|warning|critical>`, and `--action-required` to query a large
fleet without post-processing every row. JSON keeps the global fleet counts and
adds `filters` plus `matching_agents`; `agents` and `findings` contain only the
matching subset.

All other `agent --json` receipts also emit schema version `1` with namespaced
actions: `agent.keygen`, `agent.register`, `agent.rotate`, `agent.revoke`,
`agent.quarantine`, `agent.unquarantine`, `agent.bulk`, `agent.status`, and `agent.list`.
Lifecycle dry-runs include `dry_run: true` and do not update repository refs or
local key files. `agent list --json` includes `agent_count` plus the `agents`
inventory.
