# `claw git-roundtrip`

Verify Claw to Git to Claw bridge integrity for a ref.

```bash
claw git-roundtrip
claw git-roundtrip --ref-name heads/main
claw git-roundtrip --json
```

The command is a release-readiness smoke test for Git export/import plumbing. Use it alongside real Git checks such as `git fsck`, `git log`, and checkout tests.

## JSON Output

`--json` emits `schema_version: 1`, `action: "git-roundtrip"`, `verified`,
source/import refs and revisions, the exported Git ref and commit, note
settings, and a `checks` object for tree, change-linkage, and ancestry
verification.

Use global JSON errors for automation failures:

```bash
claw --error-format json git-roundtrip --json --ref-name heads/main
```

## Exit Codes

- `0`: roundtrip completed and verification passed.
- `1`: ref/path validation failed or roundtrip verification found a mismatch.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: Claw object, Git object, ref, or note read/write failed.

## Common Errors

- Source ref missing: verify with `claw branch list` or `claw log`.
- Invalid branch/import ref: use relative ref paths without `.` or `..`.
- Tree, change-linkage, or ancestry mismatch: keep Git as source of truth until the bridge discrepancy is fixed.
