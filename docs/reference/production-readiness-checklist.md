# Production Readiness Checklist

Use this checklist before opening production traffic to a new Claw deployment or after a major platform change.

Mark each item as `PASS` or `FAIL`. A `FAIL` blocks go-live until remediated or accepted through your formal exception process.

## How to use

- Scope one environment at a time (for example: `prod-us-east-1`).
- Record evidence links for each check (run output, screenshots, ticket IDs, dashboards).
- Keep this checklist with your release record.
- For repository health evidence, use `scripts/verify-repo-health.sh <repo>`
  with `CLAW_HEALTH_REPORT=<path>` to archive the doctor/repair JSON receipts.
  The report is written for both passing and failing gates and includes the
  `generated_at_ms` timestamp, target `repo_path`, `claw_bin`, command exit
  codes, failures, and raw version/doctor/repair receipts. See
  [Repository Health Report](repo-health-report.md).

## Readiness checks

| Area | Check | PASS criteria | FAIL criteria |
|---|---|---|---|
| Install | Production install path is reproducible | Install completed using documented production steps; exact version and install method are recorded; `claw --version` matches planned release | Install required ad-hoc/manual fixes, version is unknown/mismatched, or steps are not reproducible |
| Auth/TLS | Authentication is enforced for daemon access | Daemon is started with bearer auth (`--auth-profile` or `--auth-token-stdin`); unauthenticated request is rejected | Daemon accepts unauthenticated requests or auth configuration is unknown |
| Auth/TLS | Transport is encrypted in transit | TLS termination is configured at ingress/proxy or daemon TLS is enabled; certificate is valid and not expired; plaintext endpoint is not reachable from production networks | TLS is missing/misconfigured, certificate is invalid/expired, or plaintext access is exposed |
| Backups | Backup jobs run on schedule | Automated backup schedule exists, ran successfully within the required window, and latest backup artifact is present | No schedule, missed backup window, or backup artifact missing |
| Backups | Backup integrity is verified | Most recent backup passes verification (`claw admin backup verify` or equivalent process) and verification result is stored | Backup verification not executed, failed, or evidence is missing |
| Rollback | Rollback plan is tested for this release path | Operator can execute documented rollback procedure and restore service/data to previous known-good state within target time | Rollback steps are untested, incomplete, or cannot restore service in target time |
| Repository health | Machine-readable health scan is clean | `claw doctor --json --strict` passes for the target repository, and `claw repair --json plan` reports zero `summary.error_count` and zero `repairable_count` or has an approved repair/waiver record | Doctor strict mode fails, repair plan reports unhandled errors or safe repairs, or health evidence is missing |
| Compatibility | Client/server compatibility is validated | Supported client versions are tested against deployed daemon version and behavior matches published compatibility guarantees | Required client versions fail, are untested, or compatibility status is unknown |
| Security scans | Required security scans pass | Required scans (dependency, container/image, and/or static analysis per policy) completed for this release with no unapproved critical/high findings | Scans were skipped, failed, or unresolved critical/high findings exist without approved exception |
| Signed artifacts | Release artifacts are signed and verified | Published binaries/images are signed by trusted keys and signature verification was performed before deployment | Artifacts are unsigned, key trust is unknown, or verification was not performed |
| Runbooks | Operator runbooks are complete and current | Runbooks exist for startup/shutdown, backup/restore, token rotation, incident response, and rollback; each runbook was reviewed/updated for this release | Required runbooks are missing, stale, or do not match current deployment |
| SLOs | SLOs and alerting are defined and active | Availability/latency/error-budget SLOs are documented; dashboards and alerts are active; on-call knows escalation path | SLOs are undefined, alerts are missing/noisy, or escalation path is unclear |

## Go-live gate

- Go-live allowed only when all checks are `PASS` or formally waived.
- If any check is `FAIL`, record owner, remediation action, and target completion date before proceeding.
