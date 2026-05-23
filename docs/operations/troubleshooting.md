# Troubleshooting

Use this page for common operator issues in self-hosted Claw deployments.

## First-response commands

```bash
claw --version
claw admin preflight
claw admin support-bundle
claw remote list
claw auth token list
```

## Common issues

### `not in a claw repository (no .claw directory found)`

- Cause: command run outside a repository root.
- Fix: run from repo root or initialize with `claw init`.

### `no token for profile '<name>'`

- Cause: daemon or sync references an auth profile without stored token.
- Fix:

```bash
printf '%s\n' "$CLAW_TOKEN" | claw auth token set --stdin --profile <name>
```

Then retry daemon start or sync operation.

### `use only one of --auth-token, --auth-token-stdin, or --auth-profile`

- Cause: multiple daemon auth token sources are set.
- Fix: choose exactly one auth source. Prefer `--auth-profile` for production
  services or `--auth-token-stdin` for one-off automation.

### Preflight fails on TLS config

- Cause: only one of `tls.cert_path` / `tls.key_path` configured.
- Fix: set both values in `.claw/config.toml`, or clear both.

### `non-fast-forward update ... use --force`

- Cause: ref update would rewrite remote history.
- Fix: investigate divergence first; use `--force` only during controlled recovery.

### `remote '<name>' not found`

- Cause: remote alias missing in `.claw/remotes.toml`.
- Fix:

```bash
claw remote add <name> <url> --kind grpc --token-profile <profile>
```

## Support bundle workflow

Generate a bundle for incident review:

```bash
claw admin support-bundle
```

Default output location:

- `.claw/support/support-bundle-<request-id>.json`

Bundle contains:

- repo root
- active config
- HEAD summary
- ref count
- latest backup id (if present)

## Escalation checklist

- Attach support bundle.
- Include exact command and full error text.
- Include backup id used (if rollback/restore was attempted).
- Include current and previous `claw --version`.
