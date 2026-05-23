# Key Rotation and Revocation

Agent keys identify who signed capsules and evidence. Treat them as production credentials.

## Current command surface

```bash
claw agent keygen --name build-agent-01
claw agent register --name build-agent-01 --version "2026-05-11"
claw agent register --name hosted-agent --public-key <hex-ed25519-public-key>
claw agent rotate --name build-agent-01 --version "2026-05-12"
claw agent rotate --name hosted-agent --public-key <replacement-public-key>
claw agent quarantine --name build-agent-01 --reason "runner drift"
claw agent unquarantine --name build-agent-01
claw agent revoke --name build-agent-01 --reason "runner compromise"
claw agent audit
claw agent list
claw agent status build-agent-01
```

`claw agent register` generates or repairs a local Ed25519 signing key and
stores public registration metadata in the repository under `refs/agents/`. The
private key is local-only under `~/.claw/agent-keys/` using a hashed filename.
Re-running `register` for the same active name updates the version and keeps the
same valid local key when possible. If the local key is missing or mismatched,
the CLI generates a replacement key and updates the stored public registration.
Use `rotate` when replacing a key is intentional.

Use `claw agent keygen --name <agent>` to provision a local key without changing
repository trust. Use `claw agent register --public-key <hex>` or
`claw agent rotate --public-key <hex>` for agents whose private keys are managed
outside the current machine.

Use `claw agent audit` for fleet checks. It scans every `agents/*` ref and
reports active, quarantined, revoked, legacy, malformed, missing local key,
mismatched local key, and private-key-material findings. JSON output includes
fleet triage counts and per-agent `risk_level`, `action_required`, and
`recommended_action` fields so operational scripts can prioritize many agent
identities.

## Rotate

Use this operational procedure:

1. Stop jobs that sign as the affected agent.
2. Back up the repository and record the current public key prefix from
   `claw agent status <name>`. If you need the full object ID, inspect the
   `agents/<name>` ref with repository tooling before changing it.
3. Preview with `claw agent rotate --name <name> --version <new-version>
   --dry-run`.
4. Run `claw agent rotate --name <name> --version <new-version>` on the runner
   that should own the new key, or use `--public-key <hex>` for externally
   managed keys.
5. Run `claw agent status <name>` and confirm the key is `verified`.
6. Ship a small test capsule with `claw ship --intent <intent-id>
   --revision-ref heads/<branch> --agent <name> --evidence smoke=pass`.
7. Update policy reviewer IDs only if your policies pin public key IDs instead
   of stable agent IDs.
8. Preserve old registration data so old capsules remain attributable.

Prefer stable agent IDs in policy where possible. Policies that pin raw public
key IDs need an update when the public key changes.

## Quarantine

Use `claw agent quarantine` when a runner looks suspicious but you are not ready
to permanently revoke the identity:

1. Stop jobs that sign as the affected agent.
2. Preview with `claw agent quarantine --name <name> --reason <reason>
   --dry-run`.
3. Run `claw agent quarantine --name <name> --reason <reason>`.
4. Run `claw agent audit` and preserve the output with the incident record.
5. Re-run affected evidence from a trusted runner before integrating changes
   signed during the investigation window.

Quarantined agents cannot sign through `claw ship` and are omitted from
integration provenance trust checks. Use `claw agent unquarantine --name <name>`
to restore the same key after investigation, or `claw agent rotate --name
<name>` to replace the key and clear quarantine.

## Revoke

Use `claw agent revoke` to stop future signing and integration trust for a
registered agent:

1. Stop all runners that have the compromised private key.
2. Preview with `claw agent revoke --name <name> --reason <reason> --dry-run`.
3. Run `claw agent revoke --name <name> --reason <reason>`.
4. Remove the affected agent ID or public key ID from policies that currently
   trust it.
5. If your integration layer has a denylist, add the public key ID there.
6. Audit capsules signed during the suspected compromise window.
7. Re-run required checks from trusted runner keys for affected revisions.
8. Rotate any encrypted private fields or external secrets the key could access.
9. Register a replacement identity or rotate the existing identity as described
   above.

Old signatures remain cryptographically valid. Revocation means future policy
evaluation must stop trusting that signer for new decisions. The CLI enforces
that by omitting revoked registrations from integration provenance checks and by
refusing `claw ship --agent <name>` for revoked agents.

## Compromise Response

Assume any capsule signed after the compromise window began is suspect. The
signature remains useful because it identifies exactly which claims used the
compromised key. Keep the old public registration available for audit even when
policy no longer trusts it.
