# Evidence Freshness Policies

Fresh evidence is evidence that still describes the revision being integrated.

Policies should support:

- evidence references the exact revision hash
- evidence is newer than the revision
- evidence is produced by a trusted runner key
- evidence expires after a bounded duration
- evidence includes artifact or log digests for later verification
- evidence includes command and exit code
- evidence includes environment or toolchain digest

The policy layer includes fail-closed freshness checks for:

- missing or mismatched `revision_id`
- evidence created before the candidate revision
- missing or expired `expires_at`
- evidence older than the policy `max_age_ms`
- missing `runner_identity`
- missing `command` or `exit_code`
- missing both `log_digest` and `artifact_digest`

Example failure modes:

- `test` passed on an old revision
- runner key is not trusted for the repository
- evidence expired before integration
- log or artifact digest is missing, so reviewers cannot bind the claim to a reproducible record
- command is missing, so reviewers cannot reproduce the check

## v0.1 behavior

`claw ship` binds capsule evidence to the revision currently resolved by
`--revision-ref` (default `heads/main`). For branch workflows, pass the branch
or revision you intend to ship:

```bash
claw ship \
  --intent <intent-id> \
  --revision-ref heads/feature \
  --agent release-bot \
  --evidence test=pass \
  --evidence-command "cargo test --workspace" \
  --runner github-actions/release \
  --environment-digest sha256:<env> \
  --log-digest sha256:<log> \
  --evidence-expires-in-ms 86400000
```

Enable the gate on a policy:

```bash
claw policy create \
  --id release \
  --check test \
  --require-fresh-evidence \
  --trusted-runner github-actions/release \
  --evidence-max-age-ms 86400000
```

The same checks run during `claw policy eval`, fail-closed shipping when
`policy.fail_closed_ship = true`, and integration policy evaluation.

In `v0.1.x`, freshness policy validates that digests are present and tied to the
capsule evidence record. `claw provenance replay` compares replayed command
stdout/stderr against a claimed `log_digest` when one is present. Policy
evaluation itself does not store artifact bytes or compare external artifact
payloads against their digests; CI, release, or artifact systems must perform
that equality check until the artifact store is wired into policy execution.
