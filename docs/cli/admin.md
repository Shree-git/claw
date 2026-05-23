# `claw admin`

Administrative commands for production operators and release drills.

```bash
claw admin backup create
claw admin --json preflight
claw admin --json backup create
claw admin backup verify
claw admin --json backup verify --backup-id <backup-id>
claw admin rollback plan --backup-id <backup-id>
claw admin --json rollback plan --backup-id <backup-id>
claw admin rollback execute --backup-id <backup-id>
claw admin --json rollback execute --backup-id <backup-id>
claw admin migrate plan
claw admin --json migrate plan
claw admin migrate apply --dry-run
claw admin --json migrate apply --dry-run
claw admin support-bundle --out support-bundle.json
claw admin --json support-bundle --out support-bundle.json
```

Use `--dry-run` on migration commands before mutating a repository. Common failures include missing backups, unsupported object format versions, and invalid repository layout.

`--json` is intended for operator automation around preflight checks, config
migration, backup validation, and rollback drills. The preflight payload uses
`schema_version: 1`, `action: "preflight"`, an `ok` boolean, pass/warn/fail
summary counts, repository paths, and check rows with `name`, `status`, `detail`,
and `next_step`. Migration payloads use `action: "migrate.plan"` or
`action: "migrate.apply"` with `dry_run`, `applied`, `backup_id`, `target`,
`source`, `source_kind`, and `diff`. The backup and rollback JSON payloads use
`schema_version: 1` and include the exact top-level `backup_id`, verification
status, and file counts so a runbook can prove which backup was checked before
restore.

`support-bundle` writes a diagnostic JSON file for maintainer handoff. The file
uses `schema_version: 1`, `action: "support-bundle"`, a generated `request_id`,
the repository root, config snapshot, current head state, ref count, and latest
backup ID when available. Sensitive local config paths such as TLS certificate
and key paths are replaced with `<redacted:support-bundle>`, and the bundle
includes a `redactions` list. With `--json`, stdout is a receipt with
`schema_version: 1`, `action: "support-bundle"`, `written`, `path`,
`request_id`, `created_at_ms`, `refs_count`, `latest_backup_id`, and
`redaction_count`.
