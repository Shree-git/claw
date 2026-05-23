# Package Registry Strategy

Claw VCS uses `claw` as the CLI name and `claw-vcs` as the public repository/product package identity where a longer name is needed. The Cargo CLI package is configured as `claw-vcs` and publishes a `claw` binary.

| Channel | Status | Notes |
|---|---|---|
| GitHub Releases | historical artifact live; launch verification pending | `v0.1.0` exists with archives, checksums, signatures, installers, and MSI, but it predates the current launch-hardening checks. A new launch-hardening release still needs clean-environment verification before release archives are documented as launch-ready. |
| Homebrew | verified live | `Formula/claw.rb` in `shree-git/homebrew-tap` points at `v0.1.2-beta.5`; `brew install shree-git/tap/claw` installed `claw 0.1.2-beta.5` and passed release-channel smoke verification on 2026-05-23. Homebrew core did not have a `claw` formula in the local `brew info claw` check on 2026-05-11. |
| crates.io | planned | Checked on 2026-05-12: `claw-vcs` and the `claw-vcs-*` internal package names returned 404 from the crates.io crate API, while `claw`, `claw-core`, `claw-crypto`, and `claw-sync` are occupied by unrelated crates. The `claw-vcs-core` publish dry-run packaged and verified successfully; dependent packages dry-run only after their internal registry dependencies are live. Reserve or publish the full package set before documenting a crates.io install path. |
| WinGet | planned | Planned package id: `ShreeGit.ClawVCS`; checked on 2026-05-11 and no manifest path exists in `microsoft/winget-pkgs`. First publish requires manual PR to `microsoft/winget-pkgs`. |
| Windows MSI | historical artifact live; launch verification pending | `v0.1.0` MSI exists; verify the launch-hardening release on Windows before treating MSI install as launch-ready. |
| Shell installer | historical artifact live; launch verification pending | `v0.1.0` shell installer exists; keep non-pipe manual download path documented and verify the launch-hardening release before treating installer output as launch-ready. |
| PowerShell installer | historical artifact live; launch verification pending | `v0.1.0` PowerShell installer exists; keep non-pipe manual download path documented and verify the launch-hardening release on Windows before treating it as launch-ready. |
| Scoop | unsupported | Revisit after first stable Windows release. |
| Nix | unsupported | Prefer source build or manual archive install for now. |
| AUR | unsupported | Revisit after Linux adoption demand. |
| Docker image | planned | Docker, Helm, Terraform, and systemd deployment assets exist and are validated by CI, but no public OCI image is a launch channel until it is built, signed, attested, pushed, and clean-environment verified. |

Before broad announcement, verify every launch channel from a clean machine or container and record expected command output in release notes.

Latest preflight result on 2026-05-12: repository, branch-protection,
security-setting, Homebrew tap, and social-preview asset checks passed;
two low `rand` Dependabot alerts were still open on `main`, `claw-vcs` and the
`claw-vcs-*` internal package names were still unreserved on crates.io, WinGet
remained planned, GitHub social preview upload was not complete, and GitHub
Pages was not configured.

Maintainer preflight:

```bash
scripts/public-launch-preflight.sh
CLAW_PREFLIGHT_STRICT=1 CLAW_PREFLIGHT_CRATESIO_OWNER=<owner> scripts/public-launch-preflight.sh
```

The preflight checks package-name signals, repository settings, branch
protection, open Dependabot alert state, the local social preview asset, GitHub
social preview upload state, and optional GitHub Pages state. It is expected to
fail until launch-gating external actions, such as merging dependency fixes and
reserving or publishing the `claw-vcs` crates.io package set, are complete. Set
`CLAW_PREFLIGHT_REQUIRE_PAGES=1` when the launch includes a hosted landing page.
In strict mode, set `CLAW_PREFLIGHT_CRATESIO_OWNER` or
`CLAW_CRATESIO_EXPECTED_OWNER` so any live crates.io package names are verified
against the expected owner through the registry owners API.

Crates.io publish order:

```text
claw-vcs-core
claw-vcs-store
claw-vcs-patch
claw-vcs-crypto
claw-vcs-policy
claw-vcs-merge
claw-vcs-sync
claw-vcs-git
claw-vcs
```

The internal packages publish under `claw-vcs-*` names while preserving Rust
crate imports such as `claw_core` and dependency aliases such as `claw-core`.
`scripts/publish-cratesio.sh --package claw-vcs-core` is expected to pass before
reservation; it passed on 2026-05-12 and stopped before upload as expected in
dry-run mode. Later packages resolve earlier `claw-vcs-*` crates from the
registry during dry-run, so verify each one after its dependencies are live.
The default dry-run checks packages whose internal registry dependencies are
already live and skips the rest with an explicit dependency list; explicit
`--package` and `--start-at` dry-runs fail fast when their prerequisites are not
published yet.
The helper defaults to dry-run and refuses real publishing unless both
`--publish` and `CLAW_CRATESIO_PUBLISH=1` are present. Real publishing also
requires:

- a clean working tree
- `HEAD` exactly at `CLAW_CRATESIO_RELEASE_TAG` or `v<workspace version>`
- the same release tag present in the canonical remote configured by
  `CLAW_CRATESIO_REPO_URL`
- every package in the publish set at the same workspace version
- `CLAW_CRATESIO_EXPECTED_OWNER` set to the crates.io owner login or team id
  that must appear on each crate after publication

The script polls crates.io after each real publish and verifies that the
exact package version is visible and the expected owner login is attached
before continuing to the next package. Resumed publishes also verify the owner
on already-live internal dependency crates before publishing a dependent crate.
