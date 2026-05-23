# Release Governance

## Purpose
Define the current, implemented release gates and operator responsibilities for API/CLI/runtime contract safety.

## CI-Enforced

- **Contract diff gate (`contract-diff.yml`):**
  - Runs on PRs and pushes to `main`.
  - Publishes `contract-diff-summary.json` as artifact `contract-diff-summary`.
  - Fails when contract artifacts change without updates to release governance docs or `CHANGELOG.md`.
- **Release gates (`release.yml`, tag publish path):**
  - `quality` (fmt, clippy, workspace tests).
  - `security-audit-gate` (`cargo audit --deny warnings`).
  - `contract-tests-gate` (core and integration contract suites, including ops artifacts checks).
  - `artifact-smoke-gate` for Linux, macOS, and Windows release archives and generated installer assets before publish; full installer installability is verified by post-publish release-channel smoke.
  - `compatibility-matrix-gate` on Linux, macOS, and Windows.
- **Cross-version runtime checks (`cross-version-runtime.yml`):**
  - Runs on PRs and pushes to `main`.
  - Runs `cross_version_runtime_tests` (`claw-integration-tests`).
- **PR contract lane (`contract-tests.yml`):**
  - Runs on PRs, pushes to `main`, and manual dispatch.
  - Covers CLI workflow contracts (`cli_core_workflow_tests`, `cli_sync_e2e_tests`, `cli_error_contract_tests`) plus artifact, HTTP contract, cross-version, and mutating-sync load suites.
- **Deploy validation (`deploy-validation.yml`):**
  - Runs on PRs and pushes to `main`.
  - Validates container build smoke, Helm lint/template rendering, Terraform static validation, and systemd template sanity.
- **Signed artifact flow (`release.yml` + `verify-artifacts.yml`):**
  - `release.yml` signs each release artifact with `cosign sign-blob`, producing `<artifact>.sigstore.json`.
  - `release.yml` verifies checksums, Cosign signatures, GitHub artifact
    attestations, exact tag/source digest, signer workflow, and SBOM structure
    before uploading the signed artifact set to the GitHub Release.
  - `verify-artifacts.yml` verifies Sigstore bundles with `cosign verify-blob`, verifies GitHub artifact attestations with `gh attestation verify`, and fails on missing bundles.
- **Release channel smoke (`release-channel-smoke.yml`):**
  - Runs on published releases or manual dispatch.
  - Validates Linux, macOS, and Windows release archives/installers where assets exist, plus checksum consistency and Homebrew installability.
  - Uploads JSON release-verification artifacts for provenance, platform install
    channels, and tagged `cargo install --git`.
- **Nightly drill (`nightly-chaos.yml`):**
  - Scheduled daily at `03:00 UTC`.
  - Runs deterministic failure drills via `chaos_tests`.
  - Runs deterministic mutating-sync load pressure (`load_soak_tests`) and live compatibility drills (`cross_version_runtime_tests`) in serial mode.
- **Large repository capacity drill (`large-repo-drill.yml`):**
  - Scheduled weekly and manually dispatchable.
  - Runs the ignored 10k-file `backlog_gap_tests` drill separately from required PR gates so capacity regressions are visible without making every review wait on the large fixture.

## Recommended Operator Practice (Not CI-Enforced)

- Run/review `soak-24h.yml` for a full 24h soak before stable promotion.
- Run rollback drill in staging for each release candidate.
- Review release-channel smoke results before broad promotion.
- Review the latest large-repo drill result and record a follow-up if the 10k-file run exceeds its timeout.
- Record go/no-go decision with Release Owner, Tech Lead, and SRE.
- Publish release notes with migration notes, known issues, and rollback reference.
- Review nightly chaos workflow results and track remediation tasks.

## No-Silent-Breakage Policy
- Any externally visible contract change must be explicit, reviewed, and documented before release.
- Silent behavior changes that break existing clients are prohibited.
- Breakage discovered after release triggers immediate rollback or mitigation and incident review.

## Contract-Change Rules
- **Contract surface:** Public API, CLI flags/options/output, config schema, event payloads, and integration protocols.
- **Required for any change:**
  - Machine-readable contract diff summary artifact (`contract-diff-summary.json`) produced by CI.
  - Compatibility impact classification: `compatible`, `conditionally compatible`, or `breaking`.
  - Migration guidance and versioning plan for `breaking` or `conditionally compatible` changes.
- **Breaking changes:**
  - Must be behind an announced deprecation window unless security or safety critical.
  - Require explicit sign-off from product owner and platform owner.
  - Must include rollback strategy validated in staging.

## Roles and Accountability
- **Release Owner:** Runs the release process, confirms checklist, records go/no-go.
- **Service Owners:** Validate service-level readiness and runbooks.
- **SRE/Security:** Confirm SLO posture, rollback readiness, and security scan status.

## Required Artifacts Per Release
- Completed release checklist.
- Contract diff summary artifact (`contract-diff-summary`).
- Compatibility matrix artifact present and valid (`docs/reference/compatibility-matrix.json`).
- Signed release artifacts (`<artifact>`, `<artifact>.sigstore.json`) and verification record.
- Release notes with known issues and rollback point.
