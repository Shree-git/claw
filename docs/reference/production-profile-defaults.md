# Production Profile Defaults

Default values below are the baseline generated/used by Claw when `.claw/config.toml` is absent or migrated to `config_version = 1`.

## Enforcement Boundary

- **Runtime-enforced defaults:** values in this document are applied by the runtime when config is absent or migrated.
- **CI-enforced release controls (separate from runtime defaults):** `contract-diff.yml`, `release.yml` gates, signed artifact bundles (`.sigstore.json`), and scheduled `nightly-chaos.yml` execution.
- **Recommended operator practice:** run `verify-artifacts.yml` for the release tag before promotion, and review nightly chaos results during release readiness.

| Section | Key | Default |
|---|---|---|
| root | `config_version` | `1` |
| `auth` | `require_auth_for_daemon` | `true` |
| `auth` | `default_profile` | `"default"` |
| `tls` | `require_for_non_localhost` | `true` |
| `tls` | `cert_path` | unset |
| `tls` | `key_path` | unset |
| `timeouts` | `io_ms` | `10000` |
| `timeouts` | `git_bridge_ms` | `15000` |
| `timeouts` | `policy_eval_ms` | `5000` |
| `retries` | `idempotent_only` | `true` |
| `retries` | `max_attempts` | `4` |
| `retries` | `base_backoff_ms` | `100` |
| `retries` | `max_backoff_ms` | `2000` |
| `retries` | `jitter` | `true` |
| `queues` | `worker_pool_size` | `8` |
| `queues` | `queue_capacity` | `1024` |
| `queues` | `backpressure` | `true` |
| `queues` | `rate_limit_per_minute` | unset |
| `queues` | `max_push_chunk_bytes` | `8388608` |
| `queues` | `max_push_request_bytes` | `134217728` |
| `telemetry` | `structured_logs` | `true` |
| `telemetry` | `correlation_ids` | `true` |
| `telemetry` | `metrics` | `true` |
| `telemetry` | `traces` | `true` |
| `policy` | `fail_closed_integrate` | `true` |
| `policy` | `fail_closed_ship` | `true` |
| `backup` | `snapshot_interval_min` | `60` |
| `backup` | `verify_integrity_on_startup` | `true` |
| `backup` | `strict_startup_checks` | `true` |

## Notes for operators

- `auth.require_auth_for_daemon = true` means production deployments should require bearer auth.
- `tls.require_for_non_localhost = true` means you should enforce TLS when binding beyond localhost.
- `queues.rate_limit_per_minute` applies to sync service work and to repeated
  missing/invalid bearer-token failures before those requests reach a service.
- Non-local health/metrics binds are blocked in the production profile unless
  the daemon is started with `--allow-public-health`.
- Policy and backup defaults are fail-closed and integrity-focused.
- Use `claw admin preflight` to validate host/config assumptions before daemon startup.
