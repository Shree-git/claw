# Production Install

This guide installs Claw for the supported self-hosted `v0.1.x` production baseline. Use it for controlled rollouts where you own the network boundary, storage, backup policy, and upgrade process.

## Deployment model

- `claw daemon` runs in your environment and serves gRPC.
- Repository data is local under `.claw/` in the repo root.
- Clients connect through `claw sync` using gRPC remotes.
- Auth is bearer-token based (`authorization: Bearer <token>`).
- Non-local production gRPC binds require auth and TLS. Non-local health/metrics
  binds require an explicit `--allow-public-health` opt-in.

## Host prerequisites

- Claw binary installed and on `PATH`.
- Install from a channel whose current tag has a passing record in
  [install-verification-log.md](install-verification-log.md). Until a
  launch-hardening release is verified, use the source install path only for
  controlled evaluation.
- Stable storage for repository checkout plus `.claw/`.
- Service account with read/write access to the repository directory.
- Network controls allowing only approved clients to daemon port.

## Install procedure

1. Create or select the repository directory on the server.
2. Initialize if needed:

```bash
claw init
```

3. Configure daemon auth profile token:

```bash
printf '%s\n' "$CLAW_TOKEN" | claw auth token set --stdin --profile default
```

4. Run preflight:

```bash
claw admin preflight
```

5. Start daemon:

```bash
claw daemon \
  --listen 0.0.0.0:50051 \
  --health-listen 0.0.0.0:50052 \
  --allow-public-health \
  --auth-profile default \
  --tls-cert /etc/claw/tls/server.pem \
  --tls-key /etc/claw/tls/server-key.pem \
  --audit-log /var/log/claw/daemon-audit.jsonl
```

## Service manager example (systemd)

```ini
[Unit]
Description=Claw Daemon
After=network.target

[Service]
Type=simple
User=claw
Group=claw
WorkingDirectory=/srv/claw/repo
ExecStart=/usr/local/bin/claw daemon --listen 0.0.0.0:50051 --health-listen 0.0.0.0:50052 --allow-public-health --auth-profile default --tls-cert /etc/claw/tls/server.pem --tls-key /etc/claw/tls/server-key.pem --audit-log /var/log/claw/daemon-audit.jsonl
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
```

## Container and Helm Preview

Container deployment assets are present for operators who want to evaluate the daemon in Kubernetes, but a public OCI image is not a launch-ready install channel until a release publishes signed, attested images and records clean-environment verification.

Build the image locally from a checked-out release tag:

```bash
docker build -f crates/claw/deploy/container/Dockerfile -t claw-vcs:<release-tag> .
docker run --rm --entrypoint /usr/local/bin/claw claw-vcs:<release-tag> --version
```

Validate the chart before installation:

```bash
helm lint crates/claw/deploy/helm/claw
helm template claw crates/claw/deploy/helm/claw \
  --set image.repository=claw-vcs \
  --set image.tag=<release-tag>
```

When an official image is published, use the image repository and tag from that release's notes instead of assuming `ghcr.io/shree-git/claw-vcs` is available.

## TLS guidance

- If daemon is reachable beyond localhost, use TLS.
- You can terminate TLS at ingress/proxy, or configure cert/key in `.claw/config.toml`.
- `claw admin preflight` fails when TLS config is incomplete (`tls.cert_path` without `tls.key_path`, or inverse).

## Post-install checks

- `claw admin preflight` returns `PASS`.
- Daemon process remains healthy after restart.
- A client can run:

```bash
claw remote add origin http://claw-daemon.internal:50051 --kind grpc --token-profile default
claw sync pull --remote origin --ref-name heads/main
```

## Related docs

- [Upgrade and rollback](upgrade-and-rollback.md)
- [Troubleshooting](troubleshooting.md)
- [Daemon runbook](../runbooks/daemon-start-stop-health.md)
