# `claw init`

Initialize a Claw repository in the current directory.

## Examples

```bash
claw init
claw init /tmp/claw-demo
claw init --json
claw init --dry-run
```

After a successful non-JSON init, the CLI prints next-step onboarding hints for
`claw status`, `claw snapshot`, and `claw intent create`.

## JSON Output

`--json` emits `schema_version: 1` and prints whether repository state was
created or only previewed.

```json
{
  "schema_version": 1,
  "action": "init",
  "initialized": true,
  "created": true,
  "dry_run": false,
  "already_initialized": false,
  "head": "heads/main",
  "claw_dir": "/path/to/repo/.claw",
  "next_steps": [
    "claw status",
    "claw snapshot -m \"initial snapshot\"",
    "claw intent create --title \"describe the next change\""
  ],
  "path": "/path/to/repo"
}
```

`claw init --json --dry-run` uses the same shape with `initialized: false`,
`created: false`, and `dry_run: true`.

## Exit Codes

- `0`: repository initialized or dry-run completed.
- `2`: invalid CLI usage.
- `3`: repository discovery conflict.
- `5`: filesystem or permission failure.

For machine-readable failures, use:

```bash
claw --error-format json init
```

## Common Errors

- Existing `.claw/` state: choose a different path or use the existing repo.
- Permission denied creating `.claw/`: fix directory ownership or initialize in a writable path.
- Parent directory missing: create the parent first or pass an existing path.
