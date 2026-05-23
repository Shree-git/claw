# `claw diff`

Show changes between trees, refs, or revisions.

## Examples

```bash
claw diff
claw diff --from heads/main --to heads/feature
claw diff --name-only
claw diff --path crates/claw-core
claw diff --json --from <left> --to <right>
```

With no `--to`, Claw compares the selected source tree with the current
working tree. `--path` filters by path prefix, and `--name-only` prints a
compact status/path list.

## JSON Output

JSON output is intended for review tools and agents that need structured
path-level change data.

```json
{
  "schema_version": 1,
  "action": "diff",
  "from": "heads/main",
  "to": "working_tree",
  "path_filter": null,
  "change_count": 1,
  "changes": [
    {
      "path": "README.md",
      "status": "modified",
      "old_id": "4c...",
      "new_id": "8d..."
    }
  ]
}
```

For machine-readable failures, use:

```bash
claw --error-format json diff --from heads/main --to heads/feature
```

## Exit Codes

- `0`: diff completed, including an empty diff.
- `1`: unresolved ref/object or unclassified diff failure.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: object, ref, or working-tree read failure.

## Common Errors

- `cannot resolve`: verify `--from` or `--to` as a ref, hex object ID, or `clw_` display ID.
- Binary files changed: human output reports binary metadata; use `--json` for stable object IDs.
- Working tree scan fails: fix unreadable files or add generated paths to `.clawignore`.
