# Public Launch Backlog Coverage

This file maps the public-launch hardening backlog to concrete artifacts in the repository or verified GitHub state. It is an audit aid, not a substitute for the live launch checklist.

Status key:

- **Implemented**: the repository contains code, docs, tests, workflows, or assets for the item.
- **Verified**: external repository state was checked and recorded.
- **External pending**: the remaining work needs repository-owner, package-registry, release, trademark, or clean-machine access after this branch lands.
- **Not applicable**: optional item intentionally not configured yet.

## Completion Summary

The in-repository P0/P1/P2 hardening work landed through PR #4. The `main`
branch-protection review/signature requirements were restored and verified on
2026-05-21. The hardened `v0.1.2-beta.5` public release was published and
release-channel verification passed on 2026-05-23.

## P0

| # | Status | Evidence |
|---:|---|---|
| 1 | Verified | Repository remote is `Shree-git/claw-vcs`; README title is `Claw VCS`; CLI binary remains `claw`. |
| 2 | Implemented | README contains the v0.1 experimental status banner. |
| 3 | Implemented | `.gitignore` excludes OS/editor junk, local state, logs, databases, and secrets; `.DS_Store` is not tracked; strict public-launch preflight fails on ignored local junk outside approved build caches. |
| 4 | Verified | `docs/operations/public-launch-checklist.md` records full-history gitleaks and trufflehog passes plus GitHub secret scanning/push protection. |
| 5 | Implemented | `.github/workflows/ci.yml` runs formatting, clippy, tests, examples, docs, fuzz compile, `cargo-deny`, and `cargo-vet`. |
| 6 | Verified | `main` branch protection was restored and verified on 2026-05-21 through the GitHub branch-protection API and GraphQL: one approving review, stale approval dismissal, code-owner review, last-push approval, signed commits, strict required status checks, conversation resolution, no force pushes, no deletions, and admin enforcement are enabled. |
| 7 | Implemented | Workflows use SHA-pinned actions; `rg "uses: .+@(v[0-9]\|main\|master\|latest)" .github/workflows` returns no tag-only uses. |
| 8 | Implemented | Workflows default to `contents: read`; write scopes are job-local for release publishing, SAST, Scorecard, SBOM, and attestations. The release planning job uses read-only permissions on PRs. |
| 9 | Implemented | `.github/workflows/release.yml`, `.github/workflows/ci.yml`, `.github/workflows/sbom.yml`, and `.github/workflows/verify-artifacts.yml` cover artifact/SBOM attestations and verification. |
| 10 | Verified | `v0.1.2-beta.5` published release assets, Sigstore bundles, GitHub attestations, SBOM, metadata, archive smoke, shell installer smoke, and `cargo install --git` verification passed; evidence is recorded in `docs/operations/install-verification-log.md` and `/tmp/claw-release-verification/v0.1.2-beta.5.json`. |
| 11 | Implemented | `docs/security/threat-model.md`. |
| 12 | Implemented | Visibility semantics are documented in `docs/reference/known-limitations.md`; policy code supports `EncryptedMetadataRequired`, recipient envelopes, authorized/revoked recipients, and the legacy `restricted` alias. |
| 13 | Implemented | `crates/claw-sync/src/event_service.rs` implements an internal event bus; limitations document notes polling only as compatibility fallback. |
| 14 | Implemented | `crates/claw-git/tests/git_bridge_real_git.rs` and CLI Git bridge tests exercise export/import with real Git commands and roundtrip paths. |
| 15 | Implemented | `docs/reference/known-limitations.md`. |

## P1: Contributor And User Readiness

| # | Status | Evidence |
|---:|---|---|
| 16 | Implemented | `.github/ISSUE_TEMPLATE/*` and `.github/PULL_REQUEST_TEMPLATE.md`. |
| 17 | Implemented | `.github/CODEOWNERS` covers workflows, security-sensitive crates/docs, release scripts, workspace manifests, dist config, installer templates, and deployment assets. |
| 18 | Implemented | `CHANGELOG.md`. |
| 19 | Implemented | `SUPPORT.md`. |
| 20 | Implemented | `ROADMAP.md`. |
| 21 | Implemented | ADRs in `docs/adr/` cover no staging area, BLAKE3 IDs, Protobuf+COF, gRPC sync, intent/change/revision, capsules, and policy objects; `docs/adr/README.md` indexes owner/date/status, alternatives, supersession state, and implementation links. |
| 22 | Implemented | `docs/spec/object-format.md`. |
| 23 | Implemented | `tests/vectors/` includes COF, IDs, capsules, policies, patch, fail-closed, all core object type, and invalid/future COF vectors; CI and contract workflows run `tests/integration/vector_manifest_tests.rs` to validate the standalone launch vectors. |
| 24 | Implemented | `fuzz/fuzz_targets/` covers COF decode, migration-aware COF decode, object IDs, patch apply/codecs, JSON tree merge, Git import parsing, sync chunks, capsules, policy checks, and store objects across all core object types. |
| 25 | Implemented | Property tests live in `crates/*/tests/*props.rs`, `crates/claw-core/tests/serialization_props.rs`, and policy/crypto tamper tests. Core properties now cover canonical object payloads, dependency ordering/uniqueness, and store/load roundtrips across all twelve core object types. |
| 26 | Implemented | Git interop tests live in `crates/claw-git/tests/git_bridge_real_git.rs`, `tests/integration/spec_tests.rs`, and CLI Git workflow tests. |
| 27 | Implemented | Durability/crash coverage appears in `tests/integration/chaos_tests.rs`, `tests/integration/backlog_gap_tests.rs`, store corruption tests, and admin backup/rollback tests; CI and contract workflows run the deterministic chaos suite. |
| 28 | Verified | `.github/dependabot.yml` and `.github/workflows/dependency-review.yml`; `scripts/public-launch-preflight.sh` verifies Dependabot security updates are enabled and fails launch readiness on open default-branch Dependabot alerts. Strict preflight on 2026-05-20 reported no open Dependabot alerts. |
| 29 | Implemented + audit backlog | `deny.toml`; CI runs `cargo deny check` across configured release targets including macOS, Linux x86_64/aarch64, and Windows. Duplicate-version drift is currently warning-gated and documented in `docs/maintainers/dependency-policy.md`; move individual duplicate lines to documented `skip` entries or hard-deny once upstream splits are resolved. |
| 30 | Implemented + audit backlog | `supply-chain/{audits.toml,config.toml,imports.lock}`; CI runs `cargo vet`, and `tests/integration/ops_artifacts_tests.rs` validates the cargo-vet metadata and dependency-policy shape. Current vet data starts from bootstrap exemptions; replacing high-risk runtime/security exemptions with real audits remains dependency-maintenance work. |
| 31 | Implemented | `.github/workflows/sbom.yml` and release/CI SBOM attestation jobs; public release SBOM verification remains part of release-channel verification. |
| 32 | Implemented + external run state | `.github/workflows/scorecard.yml`; Scorecard runs on `main`, branch-protection changes, the weekly schedule, and manual dispatch. Pass status is verified from GitHub Actions, not from repository files alone. |
| 33 | Implemented + external ingestion | `.github/workflows/codeql.yml` and `.github/workflows/semgrep.yml`; code-scanning upload success and SARIF ingestion are verified in GitHub code scanning/PR checks, not from repository files alone. |
| 34 | Implemented | `docs/security/verifying-releases.md` documents single-asset checksum checks plus Cosign OIDC issuer/certificate identity constraints that match the verifier script and restrict blob signatures to `.github/workflows/release.yml` on the release tag. |
| 35 | Implemented | `docs/operations/upgrade-and-rollback.md`, `docs/runbooks/emergency-rollback.md`, and `docs/runbooks/backup-and-restore.md`. |

## P1: Product And Concept Clarity

| # | Status | Evidence |
|---:|---|---|
| 36 | Implemented | README uses complementary Git positioning instead of replacement language. |
| 37 | Implemented | `docs/concepts/object-model.md`, `docs/concepts/intent-change-revision.md`, `docs/concepts/capsules-and-evidence.md`, `docs/concepts/policies.md`, and `docs/cli/*.md`. |
| 38 | Implemented | README contains `What Claw VCS is not`. |
| 39 | Implemented | README and `docs/security/threat-model.md` define signed-claim limits and trust assumptions. |
| 40 | Implemented | Mermaid trust/adversary diagrams in `docs/security/threat-model.md` and `docs/concepts/claw-vs-attestations.md`. |
| 41 | Implemented | `docs/concepts/claw-vs-attestations.md`. |
| 42 | Implemented | `scripts/demo.sh` delegates to `examples/basic-demo/scripts/demo.sh`; CI runs the demo. |
| 43 | Implemented | `examples/demo-media/basic-demo.cast`, `status-screenshot.svg`, `ship-capsule-screenshot.svg`, and `command-gallery.svg`. |
| 44 | Implemented | `examples/demo-media/command-gallery.svg` covers status, log, diff, show, policy, failed integrate, successful ship, and Git export. |
| 45 | Implemented | `docs/migration/from-git.md` covers import/export, rollback, validation, and a Git feature support/lossiness matrix. |
| 46 | Implemented | `docs/workflows/solo.md`, `human-agent-pair.md`, `multiple-agents.md`, `policy-gated-integration.md`, `sensitive-paths.md`, and `release.md`. |
| 47 | Implemented | `docs/agents/integration-guide.md`, `agent-registration.md`, and `change-workflow.md`, including CLI, SDK, MCP, daemon, and event integration boundaries. |
| 48 | Implemented | `docs/agents/evidence-schema.md`. |
| 49 | Implemented | `docs/agents/evidence-freshness.md` plus freshness policy implementation/tests. |
| 50 | Implemented | `docs/agents/key-rotation-and-revocation.md` plus `claw agent keygen/register/rotate/quarantine/unquarantine/revoke/audit` support. |

## P1: CLI And Developer Experience

| # | Status | Evidence |
|---:|---|---|
| 51 | Implemented | `claw completions bash|zsh|fish|powershell|elvish`; generated from the canonical Clap command model and unit-tested for every documented shell plus the `claw completion <shell>` compatibility alias. |
| 52 | Implemented | `claw doctor` and `claw doctor --json`; docs in `docs/cli/doctor.md`. The report includes repository/config/object-format/ref/auth/TLS checks and a short `daemon_reachable` sync `Hello` probe for configured remotes. |
| 53 | Implemented | `claw version --json`; docs in `docs/cli/version.md`. |
| 54 | Implemented | `status`, `log`, `show`, `policy eval`, `diff`, and `doctor` support JSON output; docs under `docs/cli/`. |
| 55 | Implemented | JSON/human diagnostics include remediation; `docs/cli/exit-codes.md` and `docs/reference/public-interface-manifest.md` define the contract. |
| 56 | Implemented | Aliases are documented and parser-tested (`serve`, `st`, `br`, `co`, `snap`, `lg`, `d`, `cat`, `diag`, `ver`, `goal`, `chg`, create/new forms, and compatibility aliases where needed). |
| 57 | Implemented | Dry-run support for integrate, sync push, policy apply, git-import, and git-export; help/docs verified. |
| 58 | Implemented | `docs/cli/exit-codes.md` and `crates/claw/src/error.rs`. |
| 59 | Implemented | `claw init` prints next steps. |
| 60 | Implemented | Command docs exist in `docs/cli/` for init, intent, change, snapshot, ship, integrate, sync, mcp, git-export, and git-import with examples, JSON output, exit codes, and common errors. |

## P1: Daemon, Sync, And Security

| # | Status | Evidence |
|---:|---|---|
| 61 | Implemented | README and `docs/reference/production-profile-defaults.md` document production auth/TLS defaults. |
| 62 | Implemented | Auth/token redaction is covered by daemon/security tests and telemetry/support-bundle guidance. |
| 63 | Implemented | Sync request-size, stream, concurrency, retry, and rate-limit behavior is implemented in daemon/sync code and documented in security/production docs. The daemon also applies the configured per-minute limit to missing/invalid bearer-token failures before the auth interceptor lets requests reach service handlers. |
| 64 | Implemented | `crates/claw-sync/src/security.rs` models roles/scopes; daemon docs describe authorization. |
| 65 | Implemented | Audit logging for daemon sync/security paths is implemented with tracing plus `claw daemon --audit-log <path>` JSONL records, and documented in observability/security docs. Admin backup/restore keeps separate migration and verification records. |
| 66 | Implemented | mTLS flags/docs/tests exist for daemon/sync; `tests/integration/cli_sync_e2e_tests.rs` exercises a live TLS daemon that requires a client certificate. |
| 67 | Implemented | Replay protection uses principal/action/resource-scoped nonce metadata for mutating sync requests, authorizes before nonce consumption, and has sync tests proving unauthorized requests cannot poison replay state. Capsule evidence freshness separately binds evidence to exact revisions. |
| 68 | Implemented | `docs/reference/compatibility.md`, `compatibility-matrix.json`, and sync negotiation code. |
| 69 | Implemented | Remote compatibility/integration tests cover push/pull, full CLI clone, filtered CLI clone, interruption, auth, stale token rejection, protocol mismatch, and live TLS/mTLS clone behavior. Partial-clone filters are exposed for daemon-backed `sync pull` and `sync clone`; hosted `clawlab` HTTP remotes serialize the same filter shape when the remote advertises `partial-clone`. `sync push --policy <id>` provides a policy-aware push gate before upload/ref update, and hosted HTTP ref updates now carry the policy receipt only when the remote advertises `policy-aware-push`. |
| 70 | Implemented | Recipient model for encrypted capsule fields is implemented in crypto/policy/CLI and documented in agent/security docs; daemon capsule reads use case-insensitive recipient matching for redaction, and generic object sync denies private capsule object bytes unless the caller has `capsules:private-read` and a matching recipient principal. |

## P1: Release And Packaging

| # | Status | Evidence |
|---:|---|---|
| 71 | Implemented | `docs/operations/release-reproducibility.md` and release workflow metadata/signing/attestation/SBOM gates, including a signed release metadata asset plus pre-upload verification of checksums, signatures, tag source digest, signer workflow, SBOM attestations, SBOM structure, and allowlist validation for generated `cargo-dist` matrix containers/install commands before execution. |
| 72 | Implemented | `RELEASING.md` and `release.yml` include fmt, clippy, `cargo test --workspace --all-targets --locked`, audit, deny, vet, all fuzz-target smoke, dry-run, install verification, signatures, attestations, SBOM, Homebrew, Windows, JSON release-channel evidence, and rollback. |
| 73 | Verified | `docs/operations/package-registry-strategy.md` documents historical artifact availability, the verified Homebrew tap, launch-verification-pending channels, planned channels, unsupported channels, and the `claw-vcs` crates.io package set; publish helper enforces release tag/version/owner guardrails before real publishing. The 2026-05-20 strict preflight verified `claw-vcs` and all `claw-vcs-*` crates.io packages under owner `Shree-git`; the 2026-05-23 Homebrew verification installed `claw 0.1.2-beta.5` from `shree-git/tap/claw`. |
| 74 | Verified | Registry availability checks are recorded; `scripts/public-launch-preflight.sh` automates repeatable package-name/repository checks for `claw-vcs` and `claw-vcs-*`, and strict mode verifies live crates.io package owners when `CLAW_PREFLIGHT_CRATESIO_OWNER` or `CLAW_CRATESIO_EXPECTED_OWNER` is set. `docs/operations/name-clearance-evidence.md` records completed trademark, domain, social-handle, package, counsel, and final-decision evidence; `scripts/verify-name-clearance-evidence.sh` and strict preflight verified it on 2026-05-20. |
| 75 | Implemented | Install docs end with `claw --version`, `claw doctor`, and smoke test commands. |
| 76 | Implemented | README and release verification docs make manual download/verification the primary release path and keep pipe installers as post-verification convenience forms. |
| 77 | Implemented | `docs/operations/uninstall.md`. |

## P1: Code Quality And Maintainability

| # | Status | Evidence |
|---:|---|---|
| 78 | Implemented | Crate-level `//!` docs exist across workspace crates, including the CLI binary crate. |
| 79 | Implemented | Rustdoc examples exist for core, store, crypto, policy, and patch crates. |
| 80 | Implemented | `#![deny(missing_docs)]` is selectively enabled on the first public library targets: core, crypto, policy, and patch. |
| 81 | Implemented | MVP/TODO audit recorded in `docs/reference/panic-audit.md`; stale production markers were removed or documented. |
| 82 | Implemented | Criterion benchmark scaffolds and docs in `crates/*/benches/` and `docs/reference/benchmarks.md`. |
| 83 | Implemented | Large-repo scenarios are covered in `tests/integration/backlog_gap_tests.rs`, including CI-enforced synthetic workflow coverage, varied large JSON/binary/path-filter fixtures, and the ignored 10k-file drill. The 10k drill is wired to `.github/workflows/large-repo-drill.yml` for weekly/manual execution because a local run on 2026-05-12 passed in 628.87 seconds, which is too heavy for the required PR gate. |
| 84 | Implemented | Windows path/release-channel coverage is in compatibility matrix CI, Git bridge tests, and path safety tests for separators, reserved basenames, invalid Windows characters, Unicode names, names with spaces, component-length bounds, binary files, and executable bits; broader real-world validation remains called out before rollout. |
| 85 | Implemented | `docs/reference/unsafe-audit.md` documents unsafe audit status. |
| 86 | Implemented | Panic audit guidance and CI clippy panic/todo/unimplemented checks. |
| 87 | Implemented | Concurrency and load/soak tests cover multi-agent daemon pushes, ref writes, rate limits, and overload behavior; fetch/read interruption coverage lives in chaos and sync e2e tests. |
| 88 | Implemented | COF/object corruption tests in `crates/claw-core/tests/cof_corruption.rs`, loose-object and pack index/object corruption tests in `crates/claw-store/tests/store_props.rs`, and integration chaos tests. |
| 89 | Implemented | `claw-core` exposes COF version classification and migration-plan helpers; tests reject future versions and return the native v1 plan. Config compatibility checks and operator migration docs cover the current v0.1 migration surface. |
| 90 | Implemented | `docs/reference/compatibility.md` and `docs/reference/compatibility-matrix.json`. |

## P2: Polish, Community, And Adoption

| # | Status | Evidence |
|---:|---|---|
| 91 | Verified | `docs/index.html`, `docs/playground/index.html`, `docs/landing-page.md`, and the manual, SHA-pinned `.github/workflows/pages.yml`; strict preflight on 2026-05-20 reported GitHub Pages configured. |
| 92 | Implemented | README includes restrained CI/license/security badges. |
| 93 | Verified | `docs/assets/social-preview.png` is an upload-ready 1280x640 asset with source SVG; preflight and artifact tests verify size and dimensions. Strict preflight on 2026-05-20 reported the GitHub social preview image uploaded. |
| 94 | Verified | Logo overinvestment intentionally deferred; `docs/operations/name-clearance-evidence.md` records the v0.1.1 experimental launch naming decision and says to revisit trademark counsel before permanent logo, domain, or broad commercial brand investment. |
| 95 | Verified | Repository topics, including `cli`, are verified with `gh repo view` and recorded in `public-launch-checklist.md`; preflight fails on missing required topics. |
| 96 | Implemented | `examples/README.md` indexes `basic-human-workflow`, `agent-capsule`, `policy-gated-integration`, `git-roundtrip`, `sensitive-path`, `backup-restore`, demo media, and integration sketches. |
| 97 | Implemented | Persona docs under `docs/persona/`. |
| 98 | Implemented | `docs/maintainers/guide.md` and related maintainer docs. |
| 99 | Implemented | `docs/maintainers/governance.md`. |
| 100 | Not applicable | Funding file is optional; no funding channel is configured unless maintainers choose one. |
| 101 | Implemented | `.github/labels.yml`; live GitHub labels verified with `scripts/verify-github-labels.sh`, and `scripts/public-launch-preflight.sh` now gates on manifest/live label drift. |
| 102 | Implemented | `docs/reference/stability.md` and `docs/reference/public-interface-manifest.md` state the pre-1.0 experimental guarantee hierarchy. |
| 103 | Implemented | `docs/reference/deprecation-policy.md` and `docs/maintainers/deprecations.md`. |
| 104 | Implemented | `docs/reference/telemetry.md` and `docs/maintainers/telemetry.md`. |
| 105 | Implemented | ClawLab/hosted remote references are guarded by release-note caveats while the client-side hosted HTTP adapter supports Claw-native object/refs sync, capability negotiation, bearer auth, policy-aware ref update receipts, policy-compatible object transfer, and partial-clone filter requests. Auth commands no longer default to a concrete hosted endpoint. |
| 106 | Implemented | `docs/reference/data-layout.md`. |
| 107 | Implemented | `examples/backup-restore/` and backup/restore runbook. |
| 108 | Implemented | Disaster recovery and backup/rollback tests in `tests/integration/backlog_gap_tests.rs` plus CI example smoke. |
| 109 | Implemented | `examples/policy-gated-integration/scripts/failure-cases.sh` and workflow docs show missing evidence, sensitive paths, stale evidence, trust, and signer failures. |
| 110 | Implemented | `docs/concepts/agent-honesty-is-not-enough.md`. |

## Current External Blockers

The structured blocker list lives in
[external-blockers.json](external-blockers.json).

- No external owner-side launch blockers remain in `external-blockers.json` after the `v0.1.2-beta.5` release-channel verification.

Owner-only launch blockers are tracked in
<https://github.com/Shree-git/claw-vcs/issues/5>.
