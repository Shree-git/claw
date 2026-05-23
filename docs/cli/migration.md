# `claw migration`

Import existing team Git history into Claw objects.

```bash
claw migration wizard --git-dir ../repo/.git
claw migration wizard --git-dir ../repo/.git --metadata-file provider.json
claw migration wizard --git-dir ../repo/.git --read-notes --notes-ref claw
claw migration wizard --git-dir ../repo/.git --dry-run --json
```

`migration wizard` imports every Git branch under `refs/heads/*` into
`heads/migrated/*`, creates one inferred intent and change per branch head, and
links the change to the imported revision.

When `--metadata-file` is provided, the wizard accepts GitHub/GitLab-style JSON
exports for issues, pull requests, merge requests, checks, reviews, and branch
protection. It preserves the raw metadata as a Claw blob, matches PR/MR branch
names to inferred intents, carries provider links into the intent, and uses
issue numbers in branch names as a fallback.

The wizard also stores policy suggestions under `migration/policy-suggestions/*`
and writes a suggested `policies/migration-suggested` policy. Suggestions are
not enough on their own; inspect them before relying on the migrated workflow.
When provider metadata includes branch-protection review rules, the report keeps
the required approving review count, code-owner review flag, stale-review
dismissal flag, and last-push approval flag in `metadata_summary` and
`suggested_policy.review_requirements`. Because Claw policy cannot yet enforce
every hosted-provider review nuance directly, `suggested_policy.migration_warnings`
calls out gates that need human policy review.

Use `--read-notes` to preserve Claw provenance notes from `refs/notes/<notes-ref>`.
Use `--dry-run` to preview imported branch refs, inferred intent/change IDs,
metadata refs, and policy suggestion refs without writing repository objects.
With `--json`, the wizard emits schema version `1` with
`action: "migration.wizard"`, branch reports, note import counts, metadata refs,
metadata coverage summary, and the suggested policy payload. Each branch report
includes `metadata_match` so reviewers can see whether the intent was inferred
from an exact branch match, an issue-number fallback, or only the branch name.
The suggested policy payload includes required checks, reviewer identities,
branch-protection review requirements, sensitive paths, trust score, and
migration warnings.
