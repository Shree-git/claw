# `claw change`

Create and inspect implementation attempts linked to intents.

## Examples

```bash
claw change create --intent <intent-id>
claw change list
claw change list --intent <intent-id>
claw change show <change-id>
claw change --json list
```

A change cannot be created for a missing intent. Create an intent first or run
`claw intent list`.

## JSON Output

Pass `--json` before the subcommand:

```bash
claw change --json create --intent <intent-id>
claw change --json list --intent <intent-id>
claw change --json show <change-id>
```

JSON output emits schema version `1`. Create/show/status output includes
`action: "change.create"`, `"change.show"`, or `"change.status"` plus a
`change` object and stored object ID when available. List output includes
`action: "change.list"`, `change_count`, `filters`, and a `changes` array.

## Exit Codes

- `0`: command completed.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: object/ref read or write failure.

For machine-readable failures, use:

```bash
claw --error-format json change show <change-id>
```

## Common Errors

- Missing intent: run `claw intent list` or create an intent first.
- Unknown change ID: run `claw change list` or filter by intent.
- Invalid status update: check `claw change status --help` for accepted values.
