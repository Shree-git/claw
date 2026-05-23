# Claw VCS Operator Documentation

This tree is for teams evaluating or running self-hosted Claw deployments. Treat the guidance here as the supported operator baseline for `v0.1.x` controlled production rollouts, with a deliberately narrow support boundary rather than a claim of broad platform maturity.

## Start here

- [Quickstart](getting-started/quickstart.md)
- [Static landing page](index.html)
- [Browser playground](playground/index.html)
- [Landing page](landing-page.md)
- [Concepts](concepts/index.md)
- [Terminology](concepts/terminology.md)
- [Workflows](workflows/index.md)
- [Agent docs](agents/index.md)
- [Migration docs](migration/index.md)
- [Persona index](persona/index.md)
- [Beginner path](persona/beginner.md)
- [Platform operator path](persona/platform-operator.md)
- [Agent developer path](persona/agent-developer.md)
- [Security reviewer path](persona/security-reviewer.md)
- [VCS nerd cave](persona/vcs-nerd-cave.md)
- [CLI reference](cli/README.md)
- [Public interface manifest](reference/public-interface-manifest.md)
- [CLI JSON schemas](reference/cli-json-schemas.md)
- [Deprecation policy](reference/deprecation-policy.md)
- [Production install](operations/production-install.md)
- [Public launch checklist](operations/public-launch-checklist.md)
- [Public launch backlog coverage](operations/backlog-coverage.md)
- [Release verification](security/verifying-releases.md)
- [Threat model](security/threat-model.md)
- [Upgrade and rollback](operations/upgrade-and-rollback.md)
- [Disaster recovery](operations/disaster-recovery.md)
- [Troubleshooting](operations/troubleshooting.md)

## Runbooks

- [Runbook index](runbooks/README.md)
- [Daemon start, stop, and health](runbooks/daemon-start-stop-health.md)
- [Backup and restore](runbooks/backup-and-restore.md)
- [Token rotation](runbooks/token-rotation.md)
- [Emergency rollback](runbooks/emergency-rollback.md)

## Reference

- [Compatibility](reference/compatibility.md)
- [Stability reference](reference/stability.md)
- [Object and protocol stability tiers](reference/object-stability-tiers.md)
- [Data layout](reference/data-layout.md)
- [Security reference](reference/security.md)
- [Hosted remote protocol](reference/hosted-remote-protocol.md)
- [Plugin protocol v1](reference/plugin-protocol-v1.md)
- [Known limitations](reference/known-limitations.md)
- [Object format spec](spec/object-format.md)
- [Benchmarks](reference/benchmarks.md)
- [Unsafe audit](reference/unsafe-audit.md)
- [Panic audit](reference/panic-audit.md)
- [Production readiness checklist](reference/production-readiness-checklist.md)
- [Release channel report](reference/release-channel-report.md)
- [Repository health report](reference/repo-health-report.md)
- [Production profile defaults](reference/production-profile-defaults.md)
- [Telemetry policy](reference/telemetry.md)

## Maintainers

- [Governance](maintainers/governance.md)
- [Maintainer guide](maintainers/guide.md)
- [Telemetry maintainer guide](maintainers/telemetry.md)
- [Deprecation maintainer guide](maintainers/deprecations.md)
- [Dependency policy](maintainers/dependency-policy.md)

## Self-hosted-first baseline

Use this baseline unless you have stricter internal controls:

- Run `claw daemon` behind your own network perimeter.
- Require bearer auth (`--auth-profile` or `--auth-token-stdin`).
- Terminate TLS at an ingress/proxy or configure daemon TLS via `.claw/config.toml`.
- Validate environment with `claw admin preflight` before first start and after major changes.
- Create and verify metadata backups with `claw admin backup create` and `claw admin backup verify`.
- Run the backup/restore demo in `examples/backup-restore/`.
