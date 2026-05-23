# Observability and Alerting

Use this as the minimum operator baseline for production Claw deployments.

## Enforcement Boundary

- **CI-enforced:** release gates, contract-diff artifact generation, release artifact signing, and the scheduled `nightly-chaos.yml` run.
- **Operator practice:** dashboard/alert tuning, canary assessment, incident response, and promotion decisions.

## Telemetry Baseline

### Logs

- Daemon health and audit paths emit request IDs for correlation.
- Authorized gRPC actions emit `sync_audit_event` tracing records with request
  ID, principal, token ID, action, resource, outcome, and denial reason when
  available.
- Start the daemon with `--audit-log <path>` when you need durable
  authorization records. The file is append-only JSON Lines and mirrors the
  tracing audit fields.
- End-to-end structured JSON logs and distributed traces across CLI, storage,
  and Git bridge are planned hardening work; do not assume those fields exist in
  every component yet.

### Metrics (Prometheus)

The daemon exposes a Prometheus text endpoint at `/v1/metrics`. Current metrics
use daemon-local names and low-cardinality labels.

- **Latency**
  - `claw_daemon_http_request_latency_seconds` (histogram; label `endpoint`)
- **Queue capacity**
  - `claw_daemon_queue_depth` (gauge)
  - `claw_daemon_worker_pool_size` (gauge)
- **Auth failures**
  - `claw_daemon_auth_failures_total` (counter; label `reason=missing|invalid`)
- **Policy evaluation**
  - `claw_daemon_policy_eval_duration_seconds` (histogram)

### Tracing Boundaries

Distributed tracing is not yet a stable public interface. Treat request IDs and
`sync_audit_event` records, or the `--audit-log` JSONL file when configured, as
the current correlation surface.

## Starter Dashboard

Build one dashboard named **Claw Operations** with these panels:

1. **SLO and Error Budget**
   - Success rate for daemon health and gRPC requests where logs expose status.
   - Burn rate (5m and 1h windows).
2. **Latency**
   - P50/P95/P99 for `claw_daemon_http_request_latency_seconds` by endpoint.
3. **Backlog Health**
   - `claw_daemon_queue_depth` and `claw_daemon_worker_pool_size` over time.
4. **Policy Deny Rate**
   - Use audit logs for allow/deny counts until a policy decision counter is
     added.
5. **Auth Anomalies**
   - `rate(claw_daemon_auth_failures_total[5m])` split by reason.

## Starter Alerts

Tune thresholds after 2-4 weeks of baseline traffic.

- **Error budget burn (page)**
  - Condition: fast burn `>14x` over 5m and slow burn `>2x` over 1h.
  - Action: page primary on-call; create incident ticket.
- **Backlog growth (page)**
  - Condition: `claw_daemon_queue_depth` increasing for 15m.
  - Action: page primary; check daemon saturation and storage latency.
- **Policy deny spike (ticket/page)**
  - Condition: deny ratio >3x 7-day baseline for 10m from audit logs.
  - Action: ticket by default; page if user-facing failures exceed SLO.
- **Auth anomaly (page)**
  - Condition: `missing` or `invalid` failures >5x baseline for 5m.
  - Action: page primary; investigate token rollout, clock skew, or abuse.

## Runbook hooks and ownership

- **Primary owner:** Claw platform on-call (daemon + sync path).
- **Secondary owners:** security on-call (auth/policy), storage owner (I/O bottlenecks), release owner (recent deploy correlation).
- **CI signals to review during release operations:**
  - `contract-diff.yml` artifact `contract-diff-summary` when contract files change.
  - `release.yml` signing output (`<artifact>.sigstore.json`) and `verify-artifacts.yml` verification result.
  - Latest `nightly-chaos.yml` run (deterministic subset with `CHAOS_MODE=off`).
- Every alert should link to:
  - this doc,
  - [Troubleshooting](troubleshooting.md),
  - support-bundle command: `claw admin support-bundle`.
- First 10 minutes checklist:
  1. Confirm active burn/backlog/auth panel impact.
  2. Correlate `request_id` across health responses and audit logs.
  3. Identify boundary with dominant latency/error (`daemon`, storage, Git bridge, or release artifact path).
  4. Capture mitigation decision and owner in incident timeline.
- Post-incident:
  - Record which signal fired first.
  - Adjust thresholds only after confirming true positive/false positive pattern.
