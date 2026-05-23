# Git interop workflow

Git interop is experimental in the `v0.1.x` line. Pin Claw versions for any
automation that depends on import or export behavior.

## Before using the bridge

Start from a repository where both sides are easy to inspect:

```bash
git status --short
claw status
claw log --limit 5
```

Keep Git as the source of truth until your team has validated import, export,
checkout, notes, and rollback behavior with the exact Claw version you plan to
use.

## Export Claw history to Git

```bash
claw git-export --ref-name heads/main --branch claw/main
```

Export all heads:

```bash
claw git-export --all-heads
```

To write provenance into Git notes when supported by the command path:

```bash
claw git-export --ref-name heads/main --branch claw/main --git-notes
```

Validate the exported Git repository with Git itself:

```bash
git fsck --strict
git log --oneline refs/heads/claw/main
git cat-file -t refs/heads/claw/main
```

Claw writes exported loose Git objects and branch refs through same-directory
temporary files, fsyncs them, and then atomically publishes them. Branch refs
are updated after object export so Git readers do not observe a ref to a
missing object.

## Import Git history into Claw

```bash
claw git-import --git-ref refs/heads/main --ref-name heads/imported
```

Import all branches:

```bash
claw git-import --all-branches
```

When reading provenance notes is part of the migration, use the command's notes
option and record the notes ref in the migration log:

```bash
claw git-import --git-ref refs/heads/main --ref-name heads/imported --read-notes
```

## Verify a round trip

```bash
claw git-roundtrip --ref-name heads/main
```

Use round-trip checks before adopting Git interop in release automation.

## Import hosted provider metadata

Use `claw bridge import` when migration or audit work needs GitHub/GitLab PR
metadata inside Claw:

```bash
claw bridge --json import \
  --provider github \
  --file pr-42.json \
  --revision heads/main
```

For live provider metadata, use the same importer without `--file`:

```bash
claw bridge --json import \
  --provider github \
  --owner claw-org \
  --repo claw-repo \
  --pull 42 \
  --token-env GITHUB_TOKEN \
  --revision heads/main

claw bridge --json import \
  --provider gitlab \
  --project group/project \
  --pull 17 \
  --token-env GITLAB_TOKEN \
  --revision heads/main
```

The bridge maps PR/MR metadata into intents and changes, converts checks,
statuses, reviews, and Git notes into capsule evidence, and turns branch
protection required checks into Claw policy objects. Live imports store the
normalized provider response under `bridges/<provider>/imports` just like file
imports, so audits can trace Claw objects back to the fetched payload.

## Tested case log

Publish the exact Git shapes you tested with the Claw version under evaluation.
At minimum, record pass/fail evidence for:

| Case | Required evidence |
|---|---|
| Branch refs | import/export command, `git show-ref --heads`, `claw branch --json` |
| Tags | whether tags were ignored, preserved, or handled out of band |
| Git notes | notes ref name, `git notes --ref <ref> show`, Claw provenance lookup |
| Merge commits | parent count before and after round trip |
| Renames | whether content survives and whether rename identity is preserved or represented as delete/add |
| Submodules | `.gitmodules` and gitlink handling decision |
| LFS pointers | pointer file behavior and external LFS object custody decision |
| Binary files | byte-for-byte hash comparison |
| Executable bits | mode comparison after checkout |
| Symlinks | link target comparison on supported platforms |
| Unicode filenames | checkout and round-trip path comparison |
| Large files | size/hash comparison and runtime note |

## Known bridge limits

- Git author and committer metadata can be represented, but Claw intent/change
  structure is richer than Git commits.
- Git notes can carry Claw provenance, but consumers must explicitly read and
  verify those notes.
- Git branch protection and hosted CI settings can be imported from provider
  JSON or fetched live with `claw bridge import`; continuous hosted-provider
  synchronization is still experimental.
- Unsupported Git features or lossy conversions should be listed in the release
  notes for the version being evaluated.
