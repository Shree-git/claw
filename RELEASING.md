# Releasing Claw

This repo uses `cargo-dist` to build and publish:

- GitHub Release artifacts (archives, checksums)
- `claw-installer.sh` + `claw-installer.ps1`
- Windows `.msi`
- A Homebrew formula published to a tap repository

## One-time setup

### 1) Create the Homebrew tap repo

Create a GitHub repo:

- `shree-git/homebrew-tap`

Make sure it has a default branch (for example, commit a README) so Actions can push updates.

### 2) Add GitHub Actions secret for the tap

In the `shree-git/claw` repo, add a secret:

- `HOMEBREW_TAP_TOKEN`: a classic PAT with write access to `shree-git/homebrew-tap`

## Cutting a release

1. Bump the version in `Cargo.toml` (`[workspace.package].version`).
2. Commit the version bump.
3. Create and push a git tag in the form `vX.Y.Z` (example: `v0.1.0`).

Pushing the tag triggers `.github/workflows/release.yml` which builds and publishes artifacts.

## WinGet (manual, first publish)

WinGet publishing is manual until the initial package is accepted into `microsoft/winget-pkgs`.

Suggested package identifier:

- `ShreeGit.Claw`

High-level steps:

1. Ensure the GitHub Release includes the `.msi` asset.
2. Use `wingetcreate new` pointing at the MSI URL.
3. Submit the generated manifests as a PR to `microsoft/winget-pkgs`.

After the first acceptance, we can automate updates on each release tag.
