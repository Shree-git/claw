# Changelog

All public launch notes for Claw VCS live here from the `v0.1.x` line onward.
Earlier build details may appear in GitHub Releases.

## v0.1.2-beta.1

### Fixed

- Made integration daemon test harnesses reserve distinct gRPC and health
  listener ports to prevent soak-test address reuse flakes.
- Rejected daemon startup when `--listen` and `--health-listen` resolve to the
  same socket address.
- Generated mTLS test CA certificates with an explicit OpenSSL config so macOS
  compatibility tests work across OpenSSL 1.1 and 3.x.

## v0.1.1

### Added

- Public support guide in `SUPPORT.md`.
- Public roadmap in `ROADMAP.md`.
- GitHub issue forms, PR template, and code owner rules.
- Concept docs for intents, changes, revisions, capsules, evidence, and policies.
- Workflow docs for daily change work, policy-gated shipping, and Git interop.
- Agent, migration, persona, maintainer, stability, and data layout docs.
- Agent key lifecycle commands for key generation, registration, rotation, and revocation.
- Recipient-encrypted capsule private fields with policy-authorized recipient checks.
- Daemon authorization scopes, replay protection, request limits, audit logging, and production health/metrics opt-in.
- Internal event bus coverage for daemon-generated sync ref updates.
- Docker, Helm, Terraform, and systemd deployment assets with CI validation.
- SBOM, artifact attestation, signature verification, cargo-deny, cargo-vet, dependency review, SAST, Scorecard, and release-channel smoke workflows.

### Changed

- Expanded the docs index so launch docs are easier to find.
- Tightened release verification docs and gates around warning-class RustSec advisories, Cosign signatures, GitHub artifact attestations, and install-channel verification.
- Clarified planned install channels so non-live package managers are not presented as launch-ready commands.

## v0.1.0

- First public release line for controlled self-hosted deployments.
- Release artifacts, installer paths, operator docs, backup guidance, and rollback
  guidance are tracked in the repository and GitHub Releases.
