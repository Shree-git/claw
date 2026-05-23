# Concepts

Claw stores code history plus the intent, evidence, and policy data around that
history. Start here if you know Git and want the Claw object model.

## Pages

- [Intent, change, revision](intent-change-revision.md)
- [Terminology](terminology.md)
- [Capsules and evidence](capsules-and-evidence.md)
- [Policies](policies.md)
- [Object model](object-model.md)
- [Claw VCS vs attestations](claw-vs-attestations.md)
- [Agent honesty is not enough](agent-honesty-is-not-enough.md)

## Short glossary

| Term | Meaning |
|---|---|
| Intent | The goal for work, with constraints and acceptance tests. |
| Change | One attempt to satisfy an intent. |
| Revision | A recorded repository state, similar to a commit. |
| Snapshot | An atomic capture that records a tree root and the revision produced by that capture. |
| Capsule | Signed provenance and evidence for a revision. |
| Evidence | A named claim, such as `test=pass`, optionally bound to a command, runner, revision, timestamps, and digests. |
| Policy | Versioned rules that gate shipping and integration. |
| Visibility | Policy setting that controls whether private capsule metadata is public, encrypted, or required. |
| Recipient envelope | Encrypted capsule key material for one authorized recipient. |
| Fresh evidence | Evidence that matches the candidate revision and satisfies policy freshness fields. |
| Daemon role | Named bundle of daemon scopes such as reader, writer, or admin. |
| Daemon scope | Fine-grained daemon permission for sync, refs, objects, capsules, intents, changes, workstreams, or events. |
| Sync capability | Versioned feature advertised by a sync peer during compatibility negotiation. |
| Workstream | An ordered stack of related changes. |
