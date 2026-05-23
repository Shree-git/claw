# Claw CLI Hardening Notes

This page documents launch-facing CLI affordances for automation, onboarding, and support triage.

## Version Metadata

Use `claw version` for human output:

```console
$ claw version
claw 0.1.2-beta.5
```

Use `claw version --json` for scripts. The JSON shape is schema version `1` with `action: "version"`, `name`, `version`, `package`, optional `git_sha`, `object_format_version`, `sync_protocol_version`, `sync_capabilities`, `build`, `os`, and `arch`.

## Command Pages

- [`admin`](admin.md)
- [`agent`](agent.md)
- [`auth`](auth.md)
- [`branch`](branch.md)
- [`bridge`](bridge.md)
- [`checkout`](checkout.md)
- [`completions`](completions.md)
- [`daemon` / `serve`](daemon.md)
- [`diff`](diff.md)
- [`doctor`](doctor.md)
- [`evidence`](evidence.md)
- [`init`](init.md)
- [`intent`](intent.md)
- [`change`](change.md)
- [`capsule`](capsule.md)
- [`migration`](migration.md)
- [`mcp`](mcp.md)
- [`patch`](patch.md)
- [`plugin`](plugin.md)
- [`provenance`](provenance.md)
- [`snapshot`](snapshot.md)
- [`ship`](ship.md)
- [`integrate`](integrate.md)
- [`policy`](policy.md)
- [`remote`](remote.md)
- [`repair`](repair.md)
- [`review`](review.md)
- [`resolve`](resolve.md)
- [`sync`](sync.md)
- [`story`](story.md)
- [`trust`](trust.md)
- [`timeline`](timeline.md)
- [`git-export`](git-export.md)
- [`git-import`](git-import.md)
- [`git-roundtrip`](git-roundtrip.md)
- [`log`](log.md)
- [`status`](status.md)
- [`show`](show.md)
- [`version`](version.md)

## Shell Completions

Generate shell completion scripts with:

```console
claw completions bash
claw completions zsh
claw completions fish
claw completions powershell
claw completions elvish
```

`claw completion <shell>` is accepted as an alias.

## Aliases

Aliases keep common workflows short:

```console
claw completion <shell>    # alias for claw completions <shell>
claw serve                 # alias for claw daemon
claw st                    # alias for claw status
claw br                    # alias for claw branch
claw co                    # alias for claw checkout
claw snap                  # alias for claw snapshot
claw lg                    # alias for claw log
claw d                     # alias for claw diff
claw cat                   # alias for claw show
claw diag                  # alias for claw doctor
claw ver                   # alias for claw version
claw goal                  # alias for claw intent
claw chg                   # alias for claw change
claw intent create ...     # alias for claw intent new ...
claw change create ...     # alias for claw change new ...
claw branch new ...        # alias for claw branch create ...
claw branch rm ...         # alias for claw branch delete ...
```

## Review And Story

Use `claw review` to inspect work grouped by intent, change, revision, capsule,
policy, and evidence. Use `claw story export --intent <id>` to turn the same
history into a markdown audit narrative.

## Doctor

Run `claw doctor` to inspect local CLI and repository health. It checks the binary version, Git availability, object format support, current directory, repository discovery, `.claw` layout, config loading, HEAD state, ref target validity, remote config parsing, daemon auth/TLS readiness, and basic write permissions.

Use `claw doctor --json` for a schema version `1` structured report with `action: "doctor"`. Use `claw doctor --strict` when automation should fail if any check reports an error.

## JSON Output

Common workflow commands with structured output include:

```console
claw init --json
claw auth --json token list
claw status --json
claw log --json
claw diff --json
claw branch --json
claw bridge --json import --provider github --file pr.json --revision heads/main --dry-run
claw checkout --json <target>
claw snapshot --json -m "message"
claw intent --json list
claw intent --json show <intent-id>
claw intent --json create --title "..." --goal "..."
claw change --json list
claw change --json create --intent <intent-id>
claw remote --json list
claw remote --json add origin http://localhost:50051 --dry-run
claw show --json <object-or-ref>
claw capsule --json inspect <capsule-or-revision>
claw evidence --json query 'test=pass AND runner=github-actions/release'
claw plugin --json check --plugin ./target/debug/claw-policy-plugin
claw mcp serve
claw provenance --json replay --revision heads/main --dry-run
claw provenance --json attach-attestation --revision heads/main --file slsa.json
claw review --json --intent <intent-id>
claw story export --intent <intent-id> --format json
claw trust --json receipt --revision heads/main
claw timeline --json revision heads/main
claw resolve --json list
claw repair --json plan
claw repair --json apply --dry-run
claw admin --json backup verify
claw admin --json migrate apply --dry-run
claw admin --json support-bundle --out support-bundle.json
claw git-export --json --dry-run
claw git-import --json --dry-run
claw git-roundtrip --json
claw sync pull --remote origin --json
claw sync clone <remote> <path> --json
claw policy lint --json
claw policy eval <policy-id> --revision <revision> --json
claw policy apply --id default --check test --dry-run --json
claw agent --json list
```

Global runtime errors can be emitted as a JSON envelope with:

```console
claw --error-format json <command>
```

The v1 envelope includes `schema_version`, `code`, `message`, `reason`,
`request_id`, `remediation`, `exit_code`, and `details`. See
[CLI JSON Schemas](../reference/cli-json-schemas.md).
Human runtime errors also include a `request_id: req_<milliseconds>_<counter>`
line so support logs and terminal output can be correlated without switching
formats.

## Dry Runs

Dry-run support is available where the command can preview intent without committing its primary mutation:

```console
claw init --dry-run
claw branch create <name> --dry-run
claw branch delete <name> --dry-run
claw checkout <target> --dry-run
claw remote add <name> <url> --dry-run
claw remote remove <name> --dry-run
claw integrate --right <ref> --dry-run
claw intent --json policy add <intent-id> <policy-id> --dry-run
claw intent --json policy remove <intent-id> <policy-id> --dry-run
claw agent rotate --name <agent-id> --public-key <hex> --dry-run
claw agent quarantine --name <agent-id> --reason "runner drift" --dry-run
claw agent unquarantine --name <agent-id> --dry-run
claw agent revoke --name <agent-id> --reason "compromised key" --dry-run
claw repair apply --dry-run
claw admin migrate apply --dry-run
claw policy apply --id <policy-id> --dry-run
claw sync push --remote origin --ref-name heads/main --dry-run
claw git-export --git-dir /tmp/exported.git --dry-run
claw git-import --git-dir /path/to/repo/.git --dry-run
```

Commands with a command-specific JSON surface can combine dry-run preview with
`--json`, including init, branch, checkout, remote, integrate, intent policy,
agent lifecycle, admin migration, repair apply, bridge import, provenance,
policy apply, sync push, git export/import, and migration wizard.

## Onboarding After Init

After `claw init`, the CLI prints the next local workflow commands:

```console
claw status
claw snapshot -m "initial snapshot"
claw intent create --title "describe the next change"
```
