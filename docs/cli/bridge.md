# `claw bridge`

Import hosted Git provider metadata into Claw objects.

## Examples

```bash
claw bridge import --provider github --file pr-42.json --revision heads/main --dry-run
claw bridge --json import --provider gitlab --file mr-17.json --revision heads/main
claw bridge --json import \
  --provider github \
  --owner claw-org \
  --repo claw-repo \
  --pull 42 \
  --token-env GITHUB_TOKEN \
  --revision heads/main
claw bridge import \
  --provider gitlab \
  --project group/project \
  --pull 17 \
  --token-env GITLAB_TOKEN \
  --revision heads/main
```

`bridge import` consumes either a GitHub/GitLab API JSON export, a normalized
bundle, or live provider metadata fetched from the provider API. Live imports
require a pull/MR selector and may use `--token` or `--token-env`; unauthenticated
requests work only for public metadata allowed by the provider.

For GitHub live imports, pass `--owner`, `--repo`, and `--pull`. For GitLab,
pass `--project` and `--pull`, or `--owner` plus `--repo` as the project path.
Use `--base-url` for GitHub Enterprise or self-managed GitLab instances. Use
`--commit-sha` when the provider PR/MR response does not expose the commit whose
checks/statuses should be imported.

The importer maps provider data into Claw objects:

- pull requests or merge requests become an intent plus a linked change
- checks and statuses become capsule evidence for the selected revision
- reviews become review evidence
- Git notes become evidence summaries
- branch protection required checks become a Claw policy
- branch protection review settings are preserved in the JSON mapping summary

The raw provider JSON is also stored as a blob under `bridges/<provider>/imports`
so audits can trace Claw objects back to the imported provider payload.

With `--dry-run`, the importer reports the refs and capsule attachment it would
create but does not store raw provider blobs, register signing agents, attach
capsule evidence, create policies, or update bridge refs.

`--json` emits schema version `1` with `action: "bridge.import"`, the normalized
provider, dry-run flag, target revision, raw import ref, imported object refs,
evidence/note counts, and a `mapping` summary. `mapping` reports whether PR/MR
and branch-protection payloads were present, counts imported checks, statuses,
reviews, and notes, and preserves hosted review gates such as required approval
count, code-owner reviews, stale-review dismissal, and last-push approval.
