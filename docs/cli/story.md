# `claw story`

Export an intent/change history narrative for review notes, release evidence,
or handoff.

## Examples

```bash
claw story export --intent <intent-id>
claw story export --intent <intent-id> --format markdown
claw story export --intent <intent-id> --format json
```

Markdown output is meant for humans. It includes the intent goal, an executive
summary, a deterministic audit narrative, risk notes, next actions, acceptance
tests, policies, agents, linked changes, head revisions, attached capsules, and
per-evidence status details.

JSON output uses schema version `1` with `action: "story.export"` and includes
top-level `summary`, `narrative`, `intent`, and `changes` fields for tools that
need the same narrative structure without scraping Markdown. `summary` includes
change, revision, capsule, missing-capsule, signature, unsigned-capsule,
private-capsule, evidence, policy, acceptance-test, agent, and trust-posture
counts. `narrative` includes an audience summary, audit verdict, risk notes,
next actions, and a change-by-change timeline.
