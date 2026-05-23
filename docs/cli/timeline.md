# `claw timeline`

Debug ref movement, revision provenance, and current policy allowance.

## Examples

```bash
claw timeline ref
claw timeline ref --ref-name heads/main
claw timeline revision heads/main
claw timeline allowed --revision heads/main --policy default
claw timeline --json revision heads/main
```

`ref` reads the reflog for a ref. `revision` prints revision parents, patches,
capsule presence, evidence count, and signature count. `allowed` evaluates the
selected current policies against a revision and its attached capsule, then
reports which policy IDs allowed or denied it, the reflog event where the
revision first appeared, and the first observed ref event for each selected
policy. When any current policy allows the revision, `first_allowed_at_ms`
answers when that revision became allowed under the current policy set: the
later of the revision's first observed ref event and the allowing policy ref's
first observed event. The `answer` object is shaped for debugger-style
questions: `which_policy_let_this_through`,
`when_did_this_become_allowed`, `first_allowed_by_policy_id`,
`first_allowed_policy_ref`, and the current-policy scope note.

Use `--signer-agent`, `--signer-key`, and `--path` with `allowed` when a policy
needs integration context that is not stored directly on the revision.

JSON output uses schema version `1` with action values `timeline.ref`,
`timeline.revision`, and `timeline.allowed`. Each action preserves its
subcommand payload: reflog `entries`, a `revision` object, or policy allowance
counts, verdicts, `allowed`, `allowed_by_policy_ids`, `denied_by_policy_ids`,
`first_allowed_by_policy_id`, `first_allowed_policy_ref`, `first_seen_at_ms`,
`first_allowed_at_ms`, debugger `answer`, and matching `ref_events`.
Per-policy rows include `policy_first_seen_at_ms`, `policy_first_seen_ref`,
`allowed_since_ms`, and `allowed_since_basis`.
