# `claw snapshot`

Record the current working tree atomically.

## Examples

```bash
claw snapshot -m "initial implementation"
claw snapshot --change <change-id> -m "initial implementation"
claw snapshot --json -m "initial implementation"
```

There is no staging area. Ignored paths and `.claw/` local state are excluded according to repository ignore rules.

## JSON Output

`--json` emits `schema_version: 1` and a structured summary for scripts.
Revision IDs and parent IDs use the public `clw_...` object ID format.

```json
{
  "schema_version": 1,
  "action": "snapshot",
  "branch": "heads/main",
  "changed_files": null,
  "merge_resolved": false,
  "patches": 0,
  "revision_id": "clw_...",
  "snapshot_created": true
}
```

Clean no-op snapshots return `snapshot_created: false`, `reason: "clean"`,
`revision_id: null`, `merge_resolved: false`, `patches: 0`, and
`changed_files: 0`.

## Exit Codes

- `0`: snapshot recorded or no-op JSON result emitted.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: worktree scan, object write, or ref update failure.

For machine-readable failures, use:

```bash
claw --error-format json snapshot -m "initial implementation"
```

## Common Errors

- Missing `--message`: pass `-m "..."`.
- Not in a Claw repository: run `claw init` first.
- Permission denied while scanning or writing objects: fix filesystem ownership.
- Unexpected files in snapshots: review ignore rules and `.claw/` exclusions.
