# `claw log`

Show revision history.

## Examples

```bash
claw log
claw log --json
claw log --ref-name heads/main
claw log --all --limit 50
```

`claw log` walks revision parents from a starting ref. `--all` prints entries
from every branch head, and `--limit` caps the number of revisions returned.

## JSON Output

`--json` is suitable for dashboards, migration checks, and agent workflows that
need revision IDs without parsing human text.

```json
{
  "schema_version": 1,
  "action": "log",
  "ref": null,
  "all": false,
  "limit": 20,
  "tip_count": 1,
  "entry_count": 1,
  "entries": [
    {
      "author": "claw",
      "created_at_ms": 1710000000000,
      "parents": [],
      "revision_id": "7b...",
      "summary": "initial snapshot"
    }
  ]
}
```

For machine-readable failures, use:

```bash
claw --error-format json log --ref-name heads/main
```

## Exit Codes

- `0`: history was read successfully.
- `1`: unresolved ref or unclassified history-read failure.
- `2`: invalid CLI usage, such as an invalid limit value.
- `3`: not in a Claw repository.
- `5`: object/ref read failure.

## Common Errors

- Ref not found: verify the name with `claw branch list` or use `claw log --all`.
- Revision object missing or corrupt: run `claw doctor` and check the object store.
- Empty repository: record the first revision with `claw snapshot -m "initial snapshot"`.
