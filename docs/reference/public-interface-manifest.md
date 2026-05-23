# Public Interface Manifest

This document defines which interfaces are public and what stability guarantees operators can rely on.

## Stability Levels

- **Stable**: Backward-compatible within a major version. Breaking changes require a major version change and a deprecation window.
- **Beta**: Intended for production trials. Minor-version breaking changes are allowed with release-note callouts.
- **Experimental**: No compatibility guarantee. Can change or be removed at any release.

## Public Surfaces

| Surface | Scope | Stability | Contract for Operators |
|---|---|---|---|
| CLI command surface | User-facing commands, flags, positional arguments, exit codes, and non-debug stdout/stderr formats | Experimental | The documented command surface is tested and intended to move deliberately, but v0.1.x has no compatibility guarantee. Pin exact versions for automation and review release notes before upgrading. Exit codes are documented in `docs/cli/exit-codes.md`; machine-readable schemas are documented in `docs/reference/cli-json-schemas.md`. |
| Successful CLI JSON schemas | Command-specific `--json` success envelopes, schema versions, `action` values, and documented top-level fields | Experimental | Success schemas are versioned and documented in `docs/reference/cli-json-schemas.md`. Automation may rely on the listed v1 top-level fields within a pinned Claw version, should ignore additive fields, and should review release notes before upgrading until this surface is promoted. |
| Operational evidence reports | JSON reports emitted by release and repository-health verification helpers, including schema versions, top-level status fields, checked command metadata, and preserved pass/fail evidence | Experimental, versioned | Report schemas are documented in `docs/reference/release-channel-report.md` and `docs/reference/repo-health-report.md`. Automation may rely on the documented v1 top-level fields within a pinned Claw version, should preserve failed reports, and should ignore additive fields until this surface is promoted. |
| Daemon HTTP API | Versioned HTTP endpoints, request/response JSON/text shape, status code semantics, auth headers | Beta | Canonical schema artifact is `docs/reference/daemon-http-openapi-v1.json`; current v1 surface includes `/v1/health/live`, `/v1/health/ready`, `/v1/health/deps`, and `/v1/metrics` (Prometheus text). Endpoint behavior may change between minor releases, with release-note callouts and migration instructions. |
| MCP stdio server | `claw mcp serve` JSON-RPC methods, tool names, and tool input schemas | Experimental | The server is intended for local agent hosts. Tool results delegate to documented CLI JSON schemas, but tool names and schemas may change before v1.0. Use read-only mode by default and pin exact Claw versions for write-capable agents. |
| Plugin protocol v1 | Process-isolated plugin JSON-RPC envelope, initialize handshake, timeout behavior, sandbox-denied errors, and plugin compliance-check receipts | Experimental, versioned | The protocol is documented in `docs/reference/plugin-protocol-v1.md`; `claw plugin --json check --plugin <path>` emits a v1 `plugin.check` receipt for CI and release evidence. Pin Claw versions for plugins and treat additive receipt fields as non-breaking until this surface is promoted. |
| Policy schema | Policy document keys, value types, validation rules, and schema version negotiation | Experimental | Policy semantics may change before v1.0. Keep policy files under version control, pin Claw versions in gated workflows, and review release migration notes before upgrading. |
| Git interop contract | Mapping between Claw and Git refs/objects, clone/fetch/push interoperability rules, conflict behavior for bridge operations | Experimental | Behavior can change as interoperability matures; pin CLI + daemon versions for automation using this surface. |

## Explicitly Non-Public

The following are implementation details and may change without notice:

- Internal crate/module APIs in `crates/*`
- On-disk temporary files and cache layouts not marked as storage format contracts
- Debug log field names and tracing spans

## CLI Diagnostics

The CLI diagnostic contract covers both process exit codes and the global JSON error envelope.

- Exit codes are documented in `docs/cli/exit-codes.md`; they are expected to
  stay deliberate but are not a stable pre-1.0 contract.
- Machine-readable errors are emitted with `claw --error-format json <command>`.
- The JSON error envelope has stable top-level keys: `schema_version`, `code`,
  `message`, `reason`, `request_id`, `remediation`, `exit_code`, and `details`.
- Successful `--json` output is command-specific. The consolidated success
  schema reference is `docs/reference/cli-json-schemas.md`; individual command
  pages provide examples and operator notes.
- Plugin compliance receipts are command-specific success JSON. The
  `plugin.check` receipt is documented in `docs/reference/cli-json-schemas.md`
  and the underlying protocol is documented in
  `docs/reference/plugin-protocol-v1.md`.
- Operational evidence reports are helper-specific. Release-channel reports are
  documented in `docs/reference/release-channel-report.md`; repository-health
  reports are documented in `docs/reference/repo-health-report.md`.

## Change Control Expectations

- Every public-interface change must include release-note text.
- Any change that can break existing automation must include an upgrade path.
- Stable-surface removals require deprecation policy completion (`N` -> `N+1` -> `N+2`) after a surface is promoted to stable.
