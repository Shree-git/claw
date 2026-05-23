# `claw policy`

Create, apply, inspect, and evaluate repository policies.

```bash
claw policy create --id release --check ci
claw policy create \
  --id release \
  --check test \
  --require-fresh-evidence \
  --trusted-runner github-actions/release \
  --evidence-max-age-ms 86400000
claw policy create \
  --id sensitive \
  --sensitive-path secrets/ \
  --visibility encrypted-metadata-required \
  --recipient security-reviewer
claw policy create \
  --id sensitive-v2 \
  --recipient security-reviewer \
  --revoked-recipient former-reviewer
claw policy apply --id release --check ci --dry-run
claw policy lint
claw policy lint release --json
claw policy eval release --revision heads/main --json
claw policy simulate release --revision heads/main --path src/main.rs --json
claw policy simulate --policy-file candidate-policy.json --revision heads/main --json
```

`policy apply --dry-run` validates the policy definition, computes the object ID
that would be written, and skips the object write and ref update. With `--json`,
it emits schema version `1` with `action: "policy.apply"`, `dry_run`, `ref`,
`old_object`, `new_object`, and the embedded `policy`.

`policy lint` checks stored policies for dangerous or surprising shapes before
they gate real work. Findings explain what happened, why it matters, and the
next command to try. Examples include policies that enforce nothing, sensitive
paths with public-only metadata, fresh-evidence requirements without named
checks or trusted runners, quarantine lanes without path scope, malformed trust
thresholds, duplicate values, and recipients that are both authorized and
revoked. `--json` emits schema version `1` with `action: "policy.lint"`,
aggregate finding counts, and per-policy finding details.

`policy eval` and its alias `policy simulate` dry-run a policy against a
revision and capsule without writing repository state. Pass a stored policy ID,
or use `--policy-file <path>` to evaluate a candidate policy JSON/TOML file
before saving it. `--json` emits schema version `1` with
`action: "policy.eval"`, `allowed`, `error`, policy object metadata and source,
revision/capsule IDs, the evaluation context, and an ordered
`simulation` summary plus `simulation.steps` checklist explaining exactly which
gate passed or failed:
visibility, required checks, recipients, freshness, reviewers, sensitive paths,
quarantine lane, trust score, and external plugins. A denied policy exits with
the policy exit-code family documented in `exit-codes.md` after printing the JSON
explanation.

When `--path` is omitted, `policy eval` derives `context.touched_paths` from the
evaluated revision's patch objects so sensitive-path policies dry-run against the
actual revision. When `--path` is provided, those explicit paths form the
simulation context. JSON includes `context.touched_paths_source` and
`context.derived_touched_paths` so automation can tell whether path context came
from revision patches or CLI input.

If the revision has no attached/default capsule and `--capsule` is omitted, the
simulator still runs with a synthetic empty capsule. JSON reports
`capsule.source: "synthetic_missing"` and `capsule.synthetic: true`, allowing
missing evidence, signatures, trust, or private fields to appear as ordinary
failed simulation steps instead of a command-level lookup failure.

The simulation summary includes pass/fail step counts, `first_failed_step`,
`first_failure_reason`, and a compact `failed_steps` array for automation that
needs the blocking reasons without scanning every gate input.

Recipient flags require capsules to carry encrypted recipient envelopes for the
listed recipient IDs. Revoked recipient flags fail closed if a capsule includes
an envelope for that recipient. Freshness flags require revision-bound evidence
with runner, command, exit status, expiration, and digest metadata.

## Common Errors

- Lint reports `POLICY_NO_ENFORCEMENT`: add a check, reviewer, trust threshold,
  private visibility, authorized recipient, or quarantine rule.
- Lint reports `SENSITIVE_PATHS_WITH_PUBLIC_VISIBILITY`: use
  `--visibility encrypted-metadata-required` or add recipient enforcement.
- Lint reports `FRESHNESS_WITHOUT_TRUSTED_RUNNER`: add
  `--trusted-runner <ci-identity>` for release policies.
