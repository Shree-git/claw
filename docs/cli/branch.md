# `claw branch`

List, create, and delete Claw refs under `heads/`.

```bash
claw branch --json
claw branch create feature/example
claw branch create feature/example --dry-run
claw branch delete feature/example --dry-run
```

Deletion and creation dry runs report the target ref mutation without writing the ref store.

## JSON Output

`--json` emits `schema_version: 1` for list, create, and delete operations.
Branch targets use the public `clw_...` object ID format.

List:

```json
{
  "schema_version": 1,
  "action": "branch.list",
  "current": "main",
  "branch_count": 1,
  "branches": [
    {
      "name": "main",
      "ref": "heads/main",
      "current": true,
      "target": "clw_...",
      "unborn": false
    }
  ]
}
```

Create and delete receipts:

```json
{
  "schema_version": 1,
  "action": "branch.create",
  "branch": "feature/example",
  "ref": "heads/feature/example",
  "target": "clw_...",
  "dry_run": true,
  "created": false
}
```
