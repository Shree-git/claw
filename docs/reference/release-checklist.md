# Release Checklist

Use this checklist for every tag release and hotfix.

## CI-Enforced Gates

- [ ] `release.yml` `quality` job passed (`cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace --all-targets --locked`).
- [ ] `release.yml` `security-audit-gate` passed (`cargo audit --deny warnings`).
- [ ] Dependency policy passed (`cargo deny check`).
- [ ] `release.yml` `contract-tests-gate` passed (`claw`, `spec_tests`, `ops_artifacts_tests`).
- [ ] `release.yml` `compatibility-matrix-gate` passed on Linux, macOS, and Windows.
- [ ] `cross-version-runtime.yml` `cross-version-runtime-integration` job passed (`cross_version_runtime_tests`).
- [ ] `contract-diff.yml` passed and uploaded `contract-diff-summary.json` as `contract-diff-summary` artifact.
- [ ] All fuzz targets compile and pass a `-runs=1` smoke.

## Signed Artifact Flow

- [ ] `release.yml` `host` job signed each release file with `cosign sign-blob`.
- [ ] For each artifact, a Sigstore bundle exists in the release asset set: `<artifact>.sigstore.json`.
- [ ] `verify-artifacts.yml` successfully verified all artifact/signature bundles.
- [ ] GitHub artifact attestations exist for release artifacts and verify with `gh attestation verify`.
- [ ] SBOM exists in SPDX or CycloneDX form and is attached to the release.
- [ ] Checksums verify from a clean download.

## Install Verification

- [ ] GitHub release archive installs and `claw --version` matches the tag.
- [ ] Shell installer installs from a clean Linux/macOS environment.
- [ ] PowerShell installer installs from a clean Windows environment.
- [ ] Homebrew formula installs the tagged release.
- [ ] Windows MSI installs and adds `claw` to `PATH`.
- [ ] Each verified install path passes `claw doctor`, `claw init`, and `claw status`.

## Recommended Operator Checks (Not CI-Enforced)

- [ ] Rollback tested in staging with a known restore point.
- [ ] `soak-24h.yml` completed for a 24h run (scheduled or manual) and results reviewed.
- [ ] `large-repo-drill.yml` 10k-file drill completed within the 30-minute timeout, or a capacity follow-up is recorded.
- [ ] No unresolved Sev1/Sev2 incidents linked to the candidate.
- [ ] Latest `nightly-chaos.yml` run reviewed; follow-up recorded for any failures.
- [ ] Go/No-go recorded (Release Owner, Tech Lead, SRE), with release notes and rollback pointer published.
