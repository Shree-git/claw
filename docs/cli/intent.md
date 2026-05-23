# `claw intent`

Create and inspect structured goals.

## Examples

```bash
claw intent create --title "Add dark mode" --goal "Support theme toggling" \
  --acceptance-test "cargo test -p app"
claw intent list
claw intent show <intent-id>
claw intent run-acceptance <intent-id> --timeout-ms 300000
claw intent graph --format mermaid
claw intent graph --format html > intent-graph.html
claw intent update <intent-id> --status done
claw intent policy add <intent-id> ci-required
claw intent policy list <intent-id>
claw intent policy remove <intent-id> ci-required --dry-run
claw intent --json list
```

`claw intent policy add` validates that the policy ref exists before attaching
it. Policy refs can be bare policy IDs such as `ci-required` or full refs such
as `policies/ci-required`.

## JSON Output

Pass `--json` before the subcommand:

```bash
claw intent --json create --title "Add dark mode" --goal "Support theme toggling" \
  --acceptance-test "cargo test -p app"
claw intent --json list
claw intent --json show <intent-id>
claw intent --json run-acceptance <intent-id>
claw intent --json graph
claw intent --json policy add <intent-id> ci-required --dry-run
```

JSON output emits schema version `1`. Create/show/update output includes
`action: "intent.create"`, `"intent.show"`, or `"intent.update"` plus an
`intent` object and its stored object ID when available. List output includes
`action: "intent.list"`, `intent_count`, and an `intents` array. Policy
list/add/remove output uses `intent.policy.list`, `intent.policy.add`, or
`intent.policy.remove`; add/remove receipts include `dry_run`, `changed`,
`policy_ref`, and the candidate `intent`.

## Executable Acceptance Tests

Intent acceptance tests are runnable commands stored on the intent object. Run
them from the repository root with:

```bash
claw intent run-acceptance <intent-id>
```

`run-acceptance --json` emits `action: "intent.run_acceptance"`, command results
plus evidence-shaped entries with `name`, `status`, `duration_ms`, `command`,
and `exit_code`. Use `--keep-going` to run all commands after a failure, and
`--timeout-ms` to bound each command.

## Intent Graph

`claw intent graph` exports a Mermaid graph of intents, policies, agents,
acceptance tests, changes, head revisions, capsules, evidence, and blockers.
Use `--format html` for a standalone interactive graph UI with search,
node-type filtering, clickable node details, and related edges.

Use `--format json` or global `--json` for structured graph data with `action:
"intent.graph"`, counts, `nodes`, `edges`, `intents`, and `changes`. Node types
include `intent`, `policy`, `agent`, `acceptance`, `change`, `revision`,
`capsule`, `evidence`, and `blocker`.

## Exit Codes

- `0`: command completed.
- `2`: invalid CLI usage.
- `3`: not in a Claw repository.
- `5`: object/ref read or write failure.
- `10`: policy attachment validation failed.

For machine-readable failures, use:

```bash
claw --error-format json intent show <intent-id>
```

## Common Errors

- Unknown intent ID: run `claw intent list` and retry with the displayed ID.
- Unknown policy ref: run `claw policy show <policy-id>` or create the policy first.
- Invalid status: use one of the statuses accepted by the CLI help for `intent update`.
- Policy remove has no effect: the ref was not attached; use `claw intent policy list <intent-id>`.
