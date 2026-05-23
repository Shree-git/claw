# `claw review`

Review intent-scoped work as structured intent, change, revision, capsule, and
policy data.

## Examples

```bash
claw review
claw review --intent <intent-id>
claw review --change <change-id>
claw review --capsule <capsule-id>
claw review --json --intent <intent-id>
```

Human output is optimized for a quick local audit: each intent shows its goal,
acceptance-test count, policy count, linked changes, head revisions, and capsule
evidence/signature counts. It ends with a review index summary for the number
of changes and capsules in scope, plus a review summary that calls out whether
manual attention is required.

Filters can select the review by intent, by a single change, or by the capsule
attached to a head revision. JSON output preserves the same intent/change shape
and includes a `filters` object so scripts can confirm what selector was used.
It also includes `index.changes` and `index.capsules` arrays so tools can jump
directly to the review unit they care about without walking the nested intent
tree.

JSON output uses schema version `1` with `action: "review"` and includes
`intent_count`, `change_count`, `capsule_count`, `filters`, `summary`, `index`,
and `intents`. The `summary` block is designed for triage dashboards and
automation, with `review_required`, `blocked_intent_count`,
`missing_policy_count`, `missing_capsule_count`, `unsigned_capsule_count`,
`private_capsule_count`, `total_evidence_count`, and
`failing_evidence_count`. Embedded object fields follow the stability tiers in
`docs/reference/object-stability-tiers.md`.
