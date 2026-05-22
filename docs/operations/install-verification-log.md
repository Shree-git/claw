# Install Verification Log

This log records concrete install-channel checks for the public launch backlog.

## 2026-05-21

Environment:

```text
GitHub Actions release workflow
```

### v0.1.2-beta.1 Release Attempt

Command shape:

```bash
git tag -a v0.1.2-beta.1 -m "Claw VCS v0.1.2-beta.1" b08a14e1a87bd46fee485eab0aa3168f3751b073
git push origin v0.1.2-beta.1
```

Observed result:

```text
Release workflow run: https://github.com/Shree-git/claw-vcs/actions/runs/26204463019
quality: passed
security-audit-gate: passed
compatibility-matrix-gate: passed on ubuntu-22.04 and macos-latest while the run was inspected
build-local-artifacts: failed for every target at Validate dist matrix inputs
GitHub release: not created
```

Status: fail for release-channel verification. The workflow stopped before
artifact publication because the dist matrix command allowlist rejected the
generated cargo-dist installer commands:

```text
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v0.30.3/cargo-dist-installer.sh | sh
irm https://github.com/axodotdev/cargo-dist/releases/download/v0.30.3/cargo-dist-installer.ps1 | iex
```

The release-channel blocker remains open until the workflow allowlist fix lands
on `main`, a replacement launch tag is cut, artifacts are published, and
`scripts/verify-release-channel.sh <launch-tag>` passes from a clean
environment.

## 2026-05-11

Environment:

```text
Darwin arm64
```

### Source Install From Current Working Tree

Command shape:

```bash
tmp=$(mktemp -d)
cargo install --path crates/claw --locked --root "$tmp/install"
"$tmp/install/bin/claw" --version
"$tmp/install/bin/claw" doctor
mkdir "$tmp/demo"
cd "$tmp/demo"
"$tmp/install/bin/claw" init
"$tmp/install/bin/claw" status
```

Observed result:

```text
Installed package `claw-vcs v0.1.0` with binary `claw`
claw 0.1.0
Claw doctor ... Summary: 4 ok, 1 warning(s), 0 error(s), 7 skipped
Initialized claw repository
=== On branch main ===
No commits yet.
```

Status: pass for the local hardened tree.

## 2026-05-12

Environment:

```text
Darwin arm64
```

### crates.io Publish Dry-Run

Command:

```bash
scripts/publish-cratesio.sh --dry-run
```

Observed result:

```text
claw-vcs-core v0.1.0 packaged, verified, and stopped before upload because this was a dry run.
claw-vcs-store skipped until registry dependency is live: claw-vcs-core.
claw-vcs-patch skipped until registry dependency is live: claw-vcs-core.
claw-vcs-crypto skipped until registry dependency is live: claw-vcs-core.
claw-vcs-policy skipped until registry dependency is live: claw-vcs-core.
claw-vcs-merge skipped until registry dependencies are live: claw-vcs-core claw-vcs-patch claw-vcs-store.
claw-vcs-sync skipped until registry dependencies are live: claw-vcs-core claw-vcs-store claw-vcs-crypto.
claw-vcs-git skipped until registry dependencies are live: claw-vcs-core claw-vcs-store.
claw-vcs skipped until registry dependencies are live: claw-vcs-core claw-vcs-store claw-vcs-patch claw-vcs-merge claw-vcs-crypto claw-vcs-policy claw-vcs-sync claw-vcs-git.
```

Status: pass for the first publishable crate. The rest of the package-set dry-run
is intentionally blocked until the internal crates are published or reserved on
crates.io in dependency order.

### GitHub Release Archive

Checked release:

```text
Shree-git/claw-vcs v0.1.0
```

Command shape:

```bash
tmp=$(mktemp -d)
cd "$tmp"
gh release download v0.1.0 --repo Shree-git/claw-vcs \
  --pattern 'claw-aarch64-apple-darwin.tar.xz' \
  --pattern 'claw-aarch64-apple-darwin.tar.xz.sha256'
shasum -a 256 -c claw-aarch64-apple-darwin.tar.xz.sha256
tar -xf claw-aarch64-apple-darwin.tar.xz
./claw-aarch64-apple-darwin/claw --version
./claw-aarch64-apple-darwin/claw doctor
```

Observed result:

```text
claw-aarch64-apple-darwin.tar.xz: OK
claw 0.1.0
error: unrecognized subcommand 'doctor'
```

Status: checksum and binary launch pass, but this release predates the hardened `doctor` command. Do not treat `v0.1.0` artifacts as launch-verified for the current README verification flow.

### Channels Still Requiring Clean-Environment Verification

Unix clean-host helper:

```bash
CLAW_RELEASE_VERIFY_REPORT=release-verification/<launch-tag>-unix.json scripts/verify-release-channel.sh <launch-tag>
```

- GitHub release archive from the next launch-hardening release.
- `sha256.sum`, Cosign signatures, GitHub attestations, SBOM attestations, SBOM readability, and release metadata
  from the next launch-hardening release.
- Shell installer from the next launch-hardening release.
- PowerShell installer on Windows.
- Windows MSI on Windows.
- Homebrew formula after the tap points at the launch-hardening release.
- `cargo install --git https://github.com/shree-git/claw-vcs.git --tag <launch-tag> --package claw-vcs --locked` for the next launch-hardening release tag.

## Launch-Hardening Release Evidence Template

Copy this section for the next launch-hardening tag.

````md
## YYYY-MM-DD

Release tag:

```text
vX.Y.Z
```

Verifier:

```text
name / machine / OS / architecture
```

### Unix Release Channel

Command:

```bash
scripts/verify-release-channel.sh vX.Y.Z
```

Expected coverage:

- host archive download
- `sha256.sum`
- Cosign signatures and certificates
- GitHub artifact attestations
- GitHub release target commit matches the release tag commit
- SPDX SBOM readability and SBOM attestation verification
- Release metadata asset validation
- structured JSON report written to `CLAW_RELEASE_VERIFY_REPORT`
- shell installer in an isolated temporary `HOME`
- tagged `cargo install --git`
- `claw --version`
- `claw doctor`
- `claw init`
- `claw status`

Observed result:

```text
paste command output or summary
```

Evidence artifact:

```text
release-verification/<launch-tag>-unix.json or release-channel-smoke workflow artifact URL
```

Status: pass/fail

### Homebrew

Command:

```bash
CLAW_VERIFY_HOMEBREW=1 scripts/verify-release-channel.sh vX.Y.Z
```

Observed result:

```text
paste command output or summary
```

Status: pass/fail/not applicable

### Windows PowerShell Installer

Source:

```text
.github/workflows/release-channel-smoke.yml or a clean Windows host
```

Observed result:

```text
paste workflow URL or summarize the uploaded release-verification-windows-install artifact
```

Status: pass/fail

### Windows MSI

Source:

```text
.github/workflows/release-channel-smoke.yml or a clean Windows host
```

Observed result:

```text
paste workflow URL, command output, or summary
```

Status: pass/fail

### Notes

- Channels intentionally marked planned or unsupported:
- Follow-up fixes required before announcement:
````
