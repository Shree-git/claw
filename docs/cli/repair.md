# `claw repair`

Plan and apply conservative repository repairs.

## Examples

```bash
claw repair plan
claw repair --json plan
claw repair apply --dry-run
claw repair --json apply
```

`plan` runs the same deep object/ref/capsule checks used by `claw doctor
--deep` and prints repair plans for issues that have one.

`apply` only performs repairs that the health scanner marks safe for automatic
application. In `v0.1.x`, that includes:

- rebuilding missing or drifted capsule index refs
- rebuilding short `capsules/by-revision/<first16>` lookup refs
- rolling a broken ref back to the newest existing target found in its reflog
- recovering a deleted ref from its surviving reflog when the newest reflog
  target still exists locally
- regenerating intent policy audit metadata by removing policy refs that no
  longer resolve

Object restore and dependency repair plans remain manual because the local repo
cannot prove which remote or backup should supply missing object bytes.

Use `--dry-run` before applying repairs in automation.

In human-readable mode, `repair apply` also prints the remaining issue and safe
repair counts from a post-apply scan. A nonzero remaining safe repair count
means another explicit apply is still required or a manual issue is blocking an
automatic repair.

## JSON

`claw repair --json plan` emits schema version `1` with
`action: "repair.plan"`, repository scan counts, the deep-health `summary`
block, and an `issues` array. Issue entries include repair payloads when a safe
or manual plan is known.

`claw repair --json apply --dry-run` and `claw repair --json apply` emit schema
version `1` with `action: "repair.apply"`, `dry_run`, planned/applied counts,
the pre-apply deep-health `summary` block, `post_summary`,
`remaining_issue_count`, `remaining_repairable_count`, and the `planned` and
`applied` repair arrays. Dry-run receipts set the post-apply and remaining
fields to `null`; real apply receipts rescan the repository after safe repairs.
