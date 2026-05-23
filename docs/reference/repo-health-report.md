# Repository Health Report

`scripts/verify-repo-health.sh` emits an optional JSON report when
`CLAW_HEALTH_REPORT=<path>` is set. The report is intended for production
readiness and launch evidence records.

The helper writes the report for both passing and failing gates.

## Command

```bash
CLAW_HEALTH_REPORT=release-verification/repo-health.json scripts/verify-repo-health.sh <repo>
```

Use `CLAW_HEALTH_CLAW_BIN=<path>` to pin the exact `claw` binary under review.

## Schema

Schema version: `1`

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | integer | Report schema version. |
| `action` | string | Always `repo_health.verify`. |
| `generated_at_ms` | integer | Unix timestamp in milliseconds when the report was generated. |
| `ok` | boolean | `true` when all health checks passed. |
| `repo_path` | string | Absolute target repository path checked by the helper. |
| `claw_bin` | string | Binary path or command used for the check. |
| `failures` | array of strings | Empty on success; contains operator-readable failure reasons on failure. |
| `checks.version.command` | string | Always `claw version --json`. |
| `checks.version.exit_code` | integer | Exit code from the version command. |
| `checks.doctor.command` | string | Always `claw doctor --json --strict`. |
| `checks.doctor.exit_code` | integer | Exit code from the doctor command. |
| `checks.repair_plan.command` | string | Always `claw repair --json plan`. |
| `checks.repair_plan.exit_code` | integer | Exit code from the repair plan command. |
| `version` | object | Raw version JSON receipt, or parse-error metadata when no valid JSON was produced. |
| `doctor` | object | Raw doctor JSON receipt, or parse-error metadata when no valid JSON was produced. |
| `repair_plan` | object | Raw repair plan JSON receipt, or parse-error metadata when no valid JSON was produced. |

Passing reports require:

- `checks.version.exit_code == 0`
- `checks.doctor.exit_code == 0`
- `checks.repair_plan.exit_code == 0`
- version JSON has `schema_version: 1` and `action: "version"`
- doctor JSON has `schema_version: 1` and `action: "doctor"`
- repair JSON has `schema_version: 1` and `action: "repair.plan"`
- repair JSON has `summary.error_count == 0`
- repair JSON has `repairable_count == 0`

The helper exits nonzero when any requirement fails.
