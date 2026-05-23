# Release Channel Report

`scripts/verify-release-channel.sh` emits an optional JSON report when
`CLAW_RELEASE_VERIFY_REPORT=<path>` is set. The report records the release
channel checks that ran on the current host and is intended for launch, release,
and install-verification evidence.

The helper writes the report for both passing and failing gates. Failed reports
preserve the checks completed before the verifier stopped and the failing exit
status.

## Command

```bash
CLAW_RELEASE_VERIFY_REPORT=release-verification/<launch-tag>-unix.json scripts/verify-release-channel.sh <launch-tag>
```

Use `CLAW_RELEASE_REPO=<owner/repo>` to verify a non-default repository. Use
`CLAW_RELEASE_VERIFY_WORKDIR=<path>` to reuse a downloaded release bundle while
debugging.

## Schema

Schema version: `1`

| Field | Type | Meaning |
|---|---|---|
| `schemaVersion` | integer | Report schema version. |
| `action` | string | Always `release_channel.verify`. |
| `generatedAt` | string | ISO-8601 timestamp when the report was generated. |
| `ok` | boolean | `true` when the verifier exits successfully. |
| `exitStatus` | integer | Verifier process exit status preserved in the report. |
| `failureCommand` | string or null | Failed shell command captured by the verifier when a gate stops early. |
| `failureLine` | integer or null | Script line number associated with `failureCommand`. |
| `repo` | string | GitHub repository under verification. |
| `tag` | string | Release tag under verification. |
| `os` | string | Host operating system from `uname -s`. |
| `arch` | string | Host architecture from `uname -m`. |
| `expectedVersion` | string | Expected `claw --version` value derived from the tag. |
| `releaseTarget` | string | GitHub release `targetCommitish` value when available. |
| `tagCommit` | string | Commit resolved from the release tag when available. |
| `checks` | array | Ordered check receipts recorded before the verifier exited. |
| `checks[].channel` | string | Verification area, such as `release`, `provenance`, `sbom`, `checksum`, `archive`, `shell-installer`, `cargo-install-git`, or `homebrew`. |
| `checks[].name` | string | Check name within the channel. |
| `checks[].status` | string | `pass` or `skipped`. Failures stop the script and are represented by `ok: false` and `exitStatus`. |
| `checks[].details` | object | Check-specific metadata, such as asset name, SHA-256 digest, binary path, version, package count, or skip reason. |

Passing reports require:

- `ok == true`
- `exitStatus == 0`
- `action == "release_channel.verify"`
- `releaseTarget == tagCommit`
- release metadata, archive, installer, SBOM, and checksum checks are present
- each required signed asset has Cosign, SLSA provenance, and SBOM attestation checks
- archive and shell-installer binary smokes pass
- tagged `cargo install --git` passes unless explicitly skipped with `CLAW_SKIP_CARGO_INSTALL=1`
- Homebrew is checked when `CLAW_VERIFY_HOMEBREW=1` on macOS, otherwise the report records a skipped Homebrew check

Failed reports are still evidence. Preserve them with release notes or launch
handoff records so maintainers can see exactly which checks completed before
the verifier stopped. When available, `failureCommand` and `failureLine`
identify the first unhandled command failure that stopped the verifier.
