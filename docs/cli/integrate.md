# `claw integrate`

Merge policy-approved changes.

## Examples

```bash
claw integrate --right heads/feature
claw integrate --right heads/feature --dry-run
claw integrate --right heads/feature --dry-run --json
claw integrate --left heads/main --right heads/feature -m "Integrate feature"
```

Use dry-run before mutating refs or the worktree when reviewing policy decisions or conflict risk. Conflicts use the merge/conflict exit-code family documented in `exit-codes.md`.

Conflict output includes the path, codec, a plain-language reason, and the
semantic regions that collided. For example, a text merge can report that both
sides replaced `line 12`; a JSON merge can report the JSON path that both sides
touched. The same explanation is saved into merge state so `claw status` and
`claw resolve list` can show it while the merge is open.

## JSON Output

`claw integrate --json` emits a stable v1 receipt for clean integrations,
dry-runs, and conflict reports. A clean dry-run looks like:

```json
{
  "schema_version": 1,
  "action": "integrate",
  "dry_run": true,
  "clean": true,
  "left_ref": "heads/main",
  "right_ref": "heads/feature",
  "left_revision": "clw_...",
  "right_revision": "clw_...",
  "base_revision": "clw_...",
  "result_revision": null,
  "result_tree": "clw_...",
  "ref_updated": false,
  "worktree_updated": false,
  "merge_state_written": false,
  "conflict_count": 0,
  "conflicts": []
}
```

When conflicts are present, `clean` is `false`, `conflict_count` is non-zero,
and `conflicts[]` includes `path`, `codec`, a plain-language `reason`, and the
semantic `regions` that collided.

Use global JSON errors for automation failures:

```bash
claw --error-format json integrate --right heads/feature --dry-run --json
```

Inspect the resulting history afterward with:

```bash
claw log --json
claw status --json
```

## Exit Codes

- `0`: integration completed or dry-run succeeded.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: object/ref/worktree read or write failure.
- `8`: merge conflict blocks integration.
- `10`: policy evaluation denied integration.

## Common Errors

- Missing `--right`: pass the ref being integrated.
- Policy denial: run dry-run, inspect missing evidence, then rerun `claw ship` with required evidence.
- Merge conflicts: read the `reason:` and `collided regions:` lines, resolve the
  marked files, then use `claw resolve list` and `claw resolve mark <path>`.
- Dirty worktree: run `claw status`, commit/snapshot intended changes, or clean the worktree before integration.
