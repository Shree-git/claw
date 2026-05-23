# From Git to Claw

Use this flow to trial Claw beside an existing Git repository.

## Migration posture

Claw `v0.1.x` Git interop is for evaluation and controlled migration trials.
Keep Git as the source of truth until import, export, notes, checkout, and
rollback have been validated for your repository. Do not delete Git remotes or
branch protection during the trial.

## 1. Start from a clean Git checkout

```bash
git status --short
git branch --show-current
git log --oneline -5
```

Resolve or commit unrelated work before creating Claw metadata.

## 2. Initialize Claw

```bash
claw init
claw status
```

## 3. Import Git history

```bash
claw git-import --git-ref refs/heads/main --ref-name heads/imported
```

For multiple branches:

```bash
claw git-import --all-branches
```

For a team migration trial that should infer Claw intents and changes from
branches and provider metadata, use the wizard:

```bash
claw migration wizard --git-dir ../repo/.git --metadata-file provider.json --read-notes
```

The wizard imports every Git branch into `heads/migrated/*`, creates inferred
intent/change objects, preserves raw GitHub/GitLab issue and PR metadata as a
Claw blob, imports provenance notes when requested, and stores policy
suggestions for review.

If you rely on Git notes for provenance, include notes in the trial and record
the notes ref in your migration notes:

```bash
claw git-import --git-ref refs/heads/main --ref-name heads/imported --read-notes
```

## 4. Verify interop

```bash
claw git-roundtrip --ref-name heads/imported
claw log --ref-name heads/imported --limit 5
claw checkout imported --dry-run
```

Then export back to Git and validate with Git:

```bash
claw git-export --ref-name heads/imported --branch claw/imported --git-notes
git fsck --strict
git log --oneline refs/heads/claw/imported -5
```

## 5. Create new Claw-native work in a trial branch

```bash
claw intent create --title "Adopt Claw trial" --goal "Record first Claw-native change"
claw change create --intent <intent-id>
claw snapshot --change <change-id> -m "Start Claw trial"
claw ship --intent <intent-id> --revision-ref heads/main --evidence smoke=pass
```

For a branch trial, create and checkout a Claw branch after the first imported
snapshot, then use that branch in `--revision-ref`:

```bash
claw branch create claw-trial
claw checkout claw-trial
claw ship --intent <intent-id> --revision-ref heads/claw-trial --evidence smoke=pass
```

## 6. Decide adoption criteria

Before moving any team workflow, confirm:

- all expected Git branches imported
- exported Git history passes `git fsck --strict`
- representative checkout tests pass on imported and exported refs
- required provenance notes survive a round trip, if used
- policy-gated changes are blocked when evidence is missing
- `.claw/` backup and restore have been tested
- rollback has an owner and a written deadline

## Git feature support matrix

Use this matrix during migration planning. `v0.1.x` is intentionally
conservative: ordinary branch history is the supported path, while specialized
Git features must be validated before teams rely on a Claw round trip.

| Git feature | v0.1.x migration status | Validation command | Notes |
|---|---|---|---|
| Branches | Supported for import/export trials | `claw git-import --all-branches` then `git branch --list` in the export | Branch names are mapped to Claw refs and back to Git refs during export. |
| Merge commits | Supported for ordinary parent lists | `git log --merges --oneline` before and after export | Validate merge parent order on critical histories. |
| Tags | Not a stable round-trip contract in v0.1.x | `git tag --list` before and after trial export | Preserve Git tags in the original Git remote until release notes explicitly promote tag round trips. |
| Git notes | Trial support when enabled | `claw git-import --read-notes` and `claw git-export --git-notes` | Record the notes ref used for provenance and verify note contents after export. |
| File contents | Supported for text and binary blobs | `git diff --stat <old> <new>` and checksum representative files | Large binary files should be included in the trial set, not assumed from text-file success. |
| Executable bit | Supported, verify on target platforms | `git ls-tree -r HEAD` before and after export | Windows checkouts may not preserve execute semantics in the working tree. Verify tree modes, not only filesystem mode. |
| Unicode paths | Supported for valid tree entries | `git ls-tree -r --name-only HEAD` before and after export | Test the exact normalization used by your repository and operating systems. |
| Symlinks | Experimental; validate explicitly | `git ls-tree -r HEAD` and checkout tests on each supported OS | Treat symlink-heavy repos as controlled trials until platform results are recorded. |
| Submodules | Not modeled as first-class Claw dependencies in v0.1.x | `git submodule status --recursive` | Keep submodule metadata and fetch policy in Git-side runbooks. Do not expect Claw policy evaluation to understand nested repository state. |
| Git LFS | Not expanded or verified by Claw | `git lfs ls-files` and checksum hydrated files | Migrate with hydrated working trees only when your trial explicitly checks pointer files and large-object availability. |
| Signed commits/tags | Signature bytes are not a Claw trust primitive | `git verify-commit` / `git verify-tag` in the Git repo | Preserve Git signature verification in Git. Claw capsules are separate signed claims over Claw revisions and evidence. |
| Renames/copies | Represented through resulting tree state, not as Git rename metadata | `git log --follow` in Git and Claw diff checks | Do not depend on rename heuristics surviving as metadata. |
| Alternate object databases / grafts / replace refs | Unsupported for launch migration | Inspect `.git/objects/info/alternates`, `git replace -l` | Normalize history in Git before import. |
| SHA-256 Git repositories | Not part of the v0.1.x supported baseline | `git rev-parse --show-object-format` | Keep SHA-256 Git repos on Git until a release explicitly advertises support. |

Record any feature marked experimental or unsupported in the migration notes and
keep Git as the source of truth until the trial result is reviewed.

## Rollback

Keep Git as the system of record until the team has run restore and rollback
drills. If the trial stops, preserve `.claw/` for investigation, then keep using
the original Git remote.

If Claw metadata must be removed from a disposable trial checkout, archive or
delete only that checkout. Do not run cleanup commands against a production
checkout until the `.claw/` backup has been verified.
