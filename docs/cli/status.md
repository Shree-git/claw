# `claw status`

Show repository and working-tree status.

## Examples

```bash
claw status
claw status --json
claw --error-format json status
```

Status reports the current branch, merge state, pending worktree changes, and
whether there is a revision at `HEAD`. Use JSON output for scripts and agents
that need stable field names instead of human text.

## JSON Output

`--json` prints the repository state and path-level change metadata.

```json
{
  "schema_version": 1,
  "action": "status",
  "branch": "main",
  "head": null,
  "in_merge": false,
  "changes": [
    {
      "path": "README.md",
      "status": "added"
    }
  ]
}
```

For machine-readable failures, use the global JSON error format:

```bash
claw --error-format json status
```

## Exit Codes

- `0`: status was computed successfully.
- `1`: unclassified repository state failure.
- `3`: not in a Claw repository.
- `4`: repository config could not be loaded.
- `5`: object store or working-tree read failure.

## Common Errors

- No `.claw/` directory found: run `claw init` or change into an existing Claw repository.
- Corrupt or missing `HEAD`: run `claw doctor` before recording more snapshots.
- Permission denied while scanning the worktree: fix file ownership or ignore unreadable generated paths.
