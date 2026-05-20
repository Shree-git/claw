# Compatibility Reference

This page defines the current compatibility baseline for operators running controlled production Claw deployments.

## Versioning posture

- Current project status is early (`v0.1.x` line).
- Treat cross-version interoperability as explicit test-required, not implied.
- Use pinned client and daemon versions per environment.

## Cross-version posture (N and N-1)

- **N (current release line)**: Full support baseline is same-version CLI and daemon.
- **N-1 (previous release line)**: Not a default compatibility guarantee; use only as a controlled upgrade window with explicit validation in your environment.
- Sync operations run compatibility checks by default. `--compat-check`
  remains accepted for older scripts, and `--no-compat-check` is an
  emergency escape hatch for controlled recovery.
- Compatibility checks classify major/minor versions:
  - Same major + same minor: full support.
  - Same major + one minor step difference (N/N-1 or N/N+1): limited support for controlled rollout windows.
  - Anything else: unsupported.

## Wire/service compatibility

- Daemon sync `Hello` reports `server_version: 0.1.2-beta.1`.
- Current sync protocol identifier: `claw-sync/1`.
- Current object format: COF v1.
- `claw-core` exposes COF version classification and migration-plan helpers.
  v0.1 decodes native v1 objects only, rejects future versions, and has the
  code hook future old-version migrators must use.
- Sync `Hello` negotiates capabilities. Clients send their supported capability list; the daemon returns the supported intersection in daemon preference order. Empty client capabilities receive the compatibility baseline.
- Supported sync capabilities currently include `partial-clone`, `event-bus`, and `request-limits`.
- Primary transport is gRPC (`claw daemon` + `claw sync`).
- Hosted HTTP transport is planned for ClawLab-style remote integration; do not assume a hosted service is live unless release notes say so.
- Daemon HTTP schema artifact: `docs/reference/daemon-http-openapi-v1.json`.
- Current daemon health listener HTTP surfaces include `/v1/health/live`, `/v1/health/ready`, `/v1/health/deps`, and `/v1/metrics` (Prometheus text payload).

## Config compatibility

- Supported repository config schema: `config_version = 1` (`.claw/config.toml`).
- Unknown config versions fail to load.
- Use `claw admin migrate plan` and `claw admin migrate apply` during upgrades.

## CLI compatibility

- Command examples in docs target the current `v0.1.x` CLI shape.
- The current agent command surface is `keygen`, `register`, `rotate`,
  `revoke`, `status`, and `list`. Public-key import through
  `register --public-key` and `rotate --public-key` is part of the `v0.1.x`
  launch-hardening surface, but the exact on-disk agent registration schema is
  still pre-1.0.
- `claw ship` defaults `--revision-ref` to `heads/main`; branch automation
  should pass the intended revision ref explicitly.
- `claw integrate` requires `--right`.
- Policy enforcement requires the intent to reference the policy. Creating a
  policy object does not make it global.

## Object and protocol matrix

| Claw version | Reads COF | Writes COF | Sync protocol | Notes |
|---|---|---|---|---|
| `0.1.x` | v1 | v1 | `claw-sync/1` | Experimental baseline. |
| `0.2.x` | v1-v2 planned | v2 planned | `claw-sync/1` or `claw-sync/2` planned | Migration support must be documented before release. |
| `0.3.x` | v1-v3 planned | newest stable planned | negotiated planned | Remote sync hardening target. |

Future COF versions must reject unsupported future writes, document migration behavior that maps to the runtime `full`, `limited`, or `unsupported` classifications, and include test vectors before release.

## Platform compatibility

- Tagged releases may publish prebuilt binaries for macOS, Linux, and Windows.
- Self-hosted daemon baseline assumes Linux service operation, but protocol behavior is platform-independent.

## Self-hosted topology compatibility

- Supported deployment tiers and support boundaries are defined in [Self-Hosted Topology Tiers and Support Boundary](topology-tiers.md).

## Operator guidance

- Keep client and daemon on the same release whenever possible.
- Use one-minor-step combinations only during planned rollouts and verify with smoke sync checks before broad deployment.
- Run smoke sync checks after every upgrade.
- Keep one known-good previous release artifact for rollback.
