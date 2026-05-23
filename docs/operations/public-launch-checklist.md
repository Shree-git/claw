# Public Launch Checklist

Some items require repository owner access or external account access and cannot be completed by editing files in this repository alone.

Backlog item-by-item coverage is tracked in [backlog-coverage.md](backlog-coverage.md).
Owner-only launch blockers are tracked in
[GitHub issue #5](https://github.com/Shree-git/claw-vcs/issues/5).
The machine-readable blocker manifest is
[external-blockers.json](external-blockers.json).

Status as of 2026-05-21:

- GitHub repository: `Shree-git/claw-vcs`.
- Secret scanning, push protection, and Dependabot security updates are enabled.
  Strict preflight reports no open Dependabot alerts on `main`.
- Full-history local secret scans passed on 2026-05-12 for the
  `codex/public-launch-hardening` branch history:
  `gitleaks detect --source . --no-git=false --redact --no-banner` reported no
  leaks, and `trufflehog git file://$PWD --json --no-update` reported 0
  verified and 0 unverified secrets. Re-run both commands after the final
  launch commit before announcement.
- Code scanning uploads are accepted for PR #4 on 2026-05-12:
  CodeQL, Semgrep OSS, and Scorecard analyses exist for `refs/pull/4/merge`.
- PR #4 was merged to `main`.
- `main` branch protection now requires at least one approving review, stale
  approval dismissal, code-owner review, last-push approval, signed commits,
  strict required status checks, conversation resolution, no force pushes, no
  deletions, and admin enforcement. This was restored and verified through the
  GitHub branch-protection API and GraphQL on 2026-05-21.
- Repository topics were verified with `gh repo view` on 2026-05-12:
  `ai-agents`, `cli`, `developer-tools`, `provenance`, `rust`,
  `version-control`, `git`, `sigstore`, `slsa`, `supply-chain-security`, and
  `vcs`.
- Package-name checks on 2026-05-20:
  `claw-vcs` and the `claw-vcs-*` internal package names exist on crates.io
  with verified owner `Shree-git`. `claw`, `claw-core`, `claw-crypto`, and
  `claw-sync` remain occupied by unrelated crates. The planned WinGet manifest
  path `ShreeGit.ClawVCS` is still absent from `microsoft/winget-pkgs`, and
  `Formula/claw.rb` exists in `Shree-git/homebrew-tap`.
- `scripts/publish-cratesio.sh --dry-run` was run on 2026-05-12.
  `claw-vcs-core` packaged and verified successfully. The remaining internal
  crates were intentionally skipped because their `claw-vcs-*` registry
  dependencies cannot resolve until the first real publish sequence begins.
- Maintainer preflight on 2026-05-20 passed repository identity, visibility,
  topics, security settings, Dependabot alert state, crates.io owner checks,
  Homebrew tap presence, social preview asset checks, GitHub social preview
  upload state, GitHub Pages configuration, and completed name-clearance
  evidence. Branch-protection hardening drift was fixed on 2026-05-21. WinGet
  remains planned.
- Suggested repository labels are tracked in `.github/labels.yml`.
- Live GitHub labels were verified on 2026-05-12 with
  `scripts/verify-github-labels.sh`; every manifest label was present with the
  expected color and description. The public-launch preflight now runs the same
  verifier so label drift is launch-gated.
- PR #4 review conversations were resolved before merge.
- Remaining external checks: publish the launch-hardening release and complete
  clean-environment verification for each release channel after the hardened
  artifacts are published. The `v0.1.2-beta.1` release tag was pushed on
  2026-05-21, but the release workflow stopped before artifact publication
  because the dist matrix command allowlist rejected the generated cargo-dist
  installer commands.
  These are tracked in [issue #5](https://github.com/Shree-git/claw-vcs/issues/5)
  and [external-blockers.json](external-blockers.json).

Before announcement, run the maintainer preflight from an authenticated local checkout:

```bash
scripts/public-launch-preflight.sh
CLAW_PREFLIGHT_STRICT=1 CLAW_PREFLIGHT_CRATESIO_OWNER=<owner> scripts/public-launch-preflight.sh
CLAW_PREFLIGHT_HEALTH_REPO=<repo> CLAW_PREFLIGHT_HEALTH_REPORT=release-verification/repo-health.json scripts/public-launch-preflight.sh
```

For launch or production environments that already contain a Claw repository,
archive machine-readable repository health evidence with:

```bash
CLAW_HEALTH_REPORT=release-verification/repo-health.json scripts/verify-repo-health.sh <repo>
```

`CLAW_HEALTH_REPORT` is written for both passing and failing gates so launch
records can preserve the target repository path, checked binary, exit codes,
and exact doctor/repair evidence reviewed.
The public-launch preflight also runs this verifier when
`CLAW_PREFLIGHT_HEALTH_REPO=<repo>` is set, writing the report to
`CLAW_PREFLIGHT_HEALTH_REPORT` or `release-verification/repo-health.json`.

The normal preflight reports launch blockers that are still pending. Strict
mode is the broad-announcement gate: it fails until branch protection stays at
the documented review/signature posture, there are no open Dependabot alerts,
the `claw-vcs` crates.io package set is published under the expected owner, the
GitHub social preview is uploaded, and completed name/domain/social/package
evidence is recorded.

## Owner-Only Launch Handoff

These steps require repository owner, package registry, release, or account
access; they cannot be completed by editing this repository alone.

1. Cut the launch-hardening release tag, then verify the published release
   artifacts. The `v0.1.2-beta.1` tag attempt is not launch-ready; rerun after
   the release workflow allowlist fix lands on `main` and a replacement release
   tag is cut.

```bash
scripts/public-launch-preflight.sh
CLAW_PREFLIGHT_HEALTH_REPO=<repo> CLAW_PREFLIGHT_HEALTH_REPORT=release-verification/repo-health.json scripts/public-launch-preflight.sh
CLAW_RELEASE_VERIFY_REPORT=release-verification/<launch-tag>.json scripts/verify-release-channel.sh <launch-tag>
```

2. Record clean-environment verification results for every live install channel
   in [install-verification-log.md](install-verification-log.md). Prefer the
   JSON reports uploaded by `release-channel-smoke.yml` over pasted terminal
   summaries.

## Repository Identity

- [x] Rename the GitHub repository to `claw-vcs`.
- [x] Keep the binary and command name as `claw`.
- [x] Reserve or verify `claw-vcs` where package registries need an unambiguous project name.
- [x] Complete trademark/name clearance before investing in a permanent logo.
      Record evidence in [name-clearance.md](name-clearance.md).

## GitHub Repository Rules

For `main`, require:

- [x] pull request before merging
- [x] at least one approving review
- [x] stale approval dismissal
- [x] code-owner review
- [x] last-push approval
- [x] required status checks
- [x] conversation resolution
- [x] signed commits
- [x] no force pushes
- [x] no branch deletions
- [x] no bypassing except a documented emergency maintainer exception

## GitHub Security Settings

- [x] Enable GitHub secret scanning.
- [x] Run full-history `gitleaks detect --source . --no-git=false --redact`.
- [x] Run full-history `trufflehog git file://$PWD --json --no-update`.
- [x] Enable Dependabot security updates.
- [x] Confirm code scanning uploads are accepted for CodeQL, Semgrep, and Scorecard workflows.
- [x] Configure release artifact provenance attestations in `release.yml`.
- [ ] Confirm the next public release artifacts verify with `gh attestation verify --repo Shree-git/claw-vcs`.

Release/install verification evidence is tracked in [install-verification-log.md](install-verification-log.md).

## Release Channel Verification

On a clean Unix host, use the helper script for the archive, checksum,
signatures, provenance/SBOM attestations, SBOM readability, release metadata, shell installer, and tagged cargo
install path:

```bash
scripts/verify-release-channel.sh <launch-tag>
```

Before announcement, test each live channel from a clean environment:

- [ ] GitHub release archive, checksum, signatures, attestations, SBOM, and release metadata
- [ ] shell installer
- [ ] PowerShell installer
- [ ] Homebrew formula
- [ ] Windows MSI
- [ ] `cargo install --git`
- [ ] Docker/OCI image, if promoted from planned to live

Mark unavailable channels as planned or unsupported in release notes and docs.

## Repository Metadata

Suggested topics:

```text
version-control
vcs
provenance
ai-agents
supply-chain-security
cli
rust
git
sigstore
slsa
developer-tools
```

Suggested labels are tracked in [`.github/labels.yml`](../../.github/labels.yml).

## Landing Page

- [x] Add a static landing page artifact in `docs/index.html`.
- [x] Add a manual, SHA-pinned GitHub Pages deployment workflow for the
  committed `docs/` site.
- [x] If the launch should include a public website, enable GitHub Pages or
  another docs host, run the manual deployment, and verify the rendered page.

## Social Preview

Suggested card copy:

```text
Claw VCS
Intent. Evidence. Provenance.
Version control for human + AI code.
```

Upload-ready asset:

- `docs/assets/social-preview.png` (1280x640 PNG, under 1 MB)
- Uploaded in GitHub repository settings as of 2026-05-20.

Source asset:

- `docs/assets/social-preview.svg`
