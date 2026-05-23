# Terminology

Claw uses a small set of repository terms deliberately. Keep these definitions
stable in user-facing docs, CLI output, and JSON receipts unless a migration
document says otherwise.

| Term | Definition | Do not use it for |
|---|---|---|
| Intent | The structured reason work exists: goal, constraints, acceptance tests, status, and policy links. | A branch, ticket comment, implementation diff, or final repository state. |
| Change | One implementation attempt toward an intent. A change can be replaced, compared with another attempt, or linked into a workstream. | The full project history, a single file diff, or a shipped artifact. |
| Revision | A recorded repository state with parents, patches, author data, and optional links to a change, capsule, and policy evidence. | The command that captured the state or the signed provenance around it. |
| Snapshot | The atomic capture operation and compatibility object that records the tree root and revision produced by that capture. | A long-lived branch, change attempt, or human-readable release note. |
| Capsule | A signed provenance envelope for one revision, including public fields, evidence, signatures, and optional encrypted private fields. | The revision itself, the policy that evaluates it, or an external CI run. |
| Evidence | A named claim, such as `test=pass`, usually bound to a revision, command, runner, timestamps, and digests. | A policy rule, a capsule signature, or general trust in an agent. |
| Policy | Versioned in-repository rules that decide whether shipping or integration is allowed. | Evidence that a check passed or a manual operating procedure. |
| Workstream | An ordered stack of related changes, usually used for dependent work or review sequencing. | An intent, branch, merge queue, release train, or arbitrary collection of revisions. |

## Relationship

The common path is:

```text
Intent -> Change -> Revision -> Capsule -> Evidence
```

Policy evaluates that path at ship or integration time. A snapshot records the atomic capture that produced a revision. A workstream orders multiple changes when a team needs stacked or dependent review.

## Writing Rules

- Use `intent` only for the structured "why."
- Use `change` only for an implementation attempt tied to an intent.
- Use `revision` for the stored repository state, not the act of capturing it.
- Use `snapshot` for the atomic capture operation or snapshot object.
- Use `capsule` for signed provenance, not for source content.
- Use `evidence` for claims inside or attached to capsules.
- Use `policy` for rules, not for proof that the rules passed.
- Use `workstream` only when order between changes matters.
