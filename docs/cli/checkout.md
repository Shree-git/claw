# `claw checkout`

Switch branches or materialize a revision into the working tree.

```bash
claw checkout heads/main
claw checkout <revision-id>
claw checkout --json heads/main
claw checkout --dry-run heads/main
```

Checkout refuses invalid refs and reports remediation through the standard CLI error envelope when `--error-format json` is used.

## JSON Output

`--json` emits `schema_version: 1`. `target_id` uses the public `clw_...`
object ID format.

```json
{
  "schema_version": 1,
  "action": "checkout",
  "target": "main",
  "target_id": "clw_...",
  "dry_run": false,
  "checked_out": true,
  "detached": false,
  "files_written": 3,
  "updated": true
}
```

Dry runs report `checked_out: false`, `files_written: 0`, and `updated: false`.
