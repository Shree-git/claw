# Quickstart (Operators)

This quickstart is for operating a self-hosted Claw daemon with production-minded defaults.

## 1) Install and verify

Install `claw` from releases, then verify:

```bash
claw --version
claw doctor
tmpdir="$(mktemp -d)"
cd "$tmpdir"
claw init
claw status
```

## 2) Initialize repository metadata

In your repository root:

```bash
claw init
```

This creates `.claw/` and prepares local object storage.

## 3) Set an integration policy baseline

Example policy requiring test and lint evidence:

```bash
claw policy create \
  --id default \
  --visibility public \
  --check test \
  --check lint \
  --reviewer release-bot \
  --min-trust-score 0.8
```

## 4) Prepare daemon auth token

Store a token in a profile used by daemon startup:

```bash
printf '%s\n' "$CLAW_TOKEN" | claw auth token set --stdin --profile default
```

## 5) Preflight checks

Run preflight before exposing the service:

```bash
claw admin preflight
```

Expected output starts with `Preflight: PASS`.

## 6) Start daemon

Example localhost start for validation:

```bash
claw daemon --listen 127.0.0.1:50051 --auth-profile default
```

For production, run under a supervisor (for example systemd) and bind to your service interface.

## 7) Register remote from a client repo

From a client/consumer repository:

```bash
claw remote add origin http://claw-daemon.internal:50051 --kind grpc --token-profile default
claw sync pull --remote origin --ref-name heads/main
```

If `heads/main` does not yet exist remotely, transport/auth is still validated when you get `Remote ref heads/main not found`.

## Next docs

- [Production install](../operations/production-install.md)
- [Daemon runbook](../runbooks/daemon-start-stop-health.md)
- [Backup and restore runbook](../runbooks/backup-and-restore.md)
