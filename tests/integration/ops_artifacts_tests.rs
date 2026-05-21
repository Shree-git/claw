use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workspace_path(relative: &str) -> PathBuf {
    workspace_root().join(relative)
}

fn read_workspace_file(relative: &str) -> String {
    let path = workspace_path(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn compatibility_matrix_json_is_valid_and_has_required_keys() {
    let raw = read_workspace_file("docs/reference/compatibility-matrix.json");
    let json: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("compatibility matrix must parse as JSON: {err}"));

    let root = json
        .as_object()
        .expect("compatibility matrix root must be a JSON object");

    for key in [
        "schemaVersion",
        "lastUpdated",
        "components",
        "supportLevels",
        "releases",
        "relationshipRules",
    ] {
        assert!(
            root.contains_key(key),
            "compatibility matrix missing top-level key: {key}"
        );
    }

    let releases = root
        .get("releases")
        .and_then(Value::as_array)
        .expect("compatibility matrix releases must be an array");

    let declared_support_levels = root
        .get("supportLevels")
        .and_then(Value::as_array)
        .expect("compatibility matrix supportLevels must be an array");

    let allowed_support_levels: HashSet<&str> =
        ["full", "limited", "unsupported"].into_iter().collect();

    let mut support_levels = HashSet::new();
    for (idx, level) in declared_support_levels.iter().enumerate() {
        let level = level
            .as_str()
            .unwrap_or_else(|| panic!("support level at index {idx} must be a string"));
        assert!(
            allowed_support_levels.contains(level),
            "support level at index {idx} is not allowed: {level}"
        );
        assert!(
            support_levels.insert(level.to_string()),
            "support level is duplicated: {level}"
        );
    }

    assert!(
        !releases.is_empty(),
        "compatibility matrix releases must not be empty"
    );

    let mut release_names = HashSet::new();
    let mut release_order = Vec::with_capacity(releases.len());

    for (idx, release) in releases.iter().enumerate() {
        let release_obj = release
            .as_object()
            .unwrap_or_else(|| panic!("release at index {idx} must be an object"));

        let release_name = release_obj
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("release at index {idx} must include string key: name"));

        assert!(
            release_names.insert(release_name.to_string()),
            "release name is duplicated: {release_name}"
        );
        release_order.push(release_name.to_string());

        for key in [
            "cliVersion",
            "daemonVersion",
            "policySchemaVersion",
            "storageFormatVersion",
        ] {
            assert!(
                release_obj.contains_key(key),
                "release at index {idx} missing key: {key}"
            );
        }

        let compatibility = release_obj
            .get("compatibility")
            .and_then(Value::as_array)
            .unwrap_or_else(|| {
                panic!("release at index {idx} must include array key: compatibility")
            });

        for (compat_idx, entry) in compatibility.iter().enumerate() {
            let entry_obj = entry.as_object().unwrap_or_else(|| {
                panic!(
                    "compatibility entry at release index {idx}, entry index {compat_idx} must be an object"
                )
            });

            let support = entry_obj
                .get("support")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "compatibility entry at release index {idx}, entry index {compat_idx} must include string key: support"
                    )
                });

            assert!(
                support_levels.contains(support),
                "compatibility entry at release index {idx}, entry index {compat_idx} has unsupported support level: {support}"
            );

            let target_release = entry_obj
                .get("targetRelease")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "compatibility entry at release index {idx}, entry index {compat_idx} must include string key: targetRelease"
                    )
                });

            assert!(
                !target_release.is_empty(),
                "compatibility entry at release index {idx}, entry index {compat_idx} has empty targetRelease"
            );
        }
    }

    let release_name_to_index: std::collections::HashMap<&str, usize> = release_order
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.as_str(), idx))
        .collect();

    assert!(
        release_name_to_index.contains_key("N"),
        "releases must include base release N"
    );

    for (earlier, later) in [("N", "N+1"), ("N+1", "N+2")] {
        if let (Some(earlier_idx), Some(later_idx)) = (
            release_name_to_index.get(earlier),
            release_name_to_index.get(later),
        ) {
            assert!(
                earlier_idx < later_idx,
                "release order must progress forward: {earlier} must come before {later}"
            );
        }
    }

    assert!(
        !release_name_to_index.contains_key("N+2") || release_name_to_index.contains_key("N+1"),
        "release N+2 requires N+1 to be present"
    );

    for (idx, release) in releases.iter().enumerate() {
        let release_obj = release
            .as_object()
            .unwrap_or_else(|| panic!("release at index {idx} must be an object"));
        let compatibility = release_obj
            .get("compatibility")
            .and_then(Value::as_array)
            .unwrap_or_else(|| {
                panic!("release at index {idx} must include array key: compatibility")
            });

        for (compat_idx, entry) in compatibility.iter().enumerate() {
            let entry_obj = entry.as_object().unwrap_or_else(|| {
                panic!(
                    "compatibility entry at release index {idx}, entry index {compat_idx} must be an object"
                )
            });
            let target_release = entry_obj
                .get("targetRelease")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!(
                        "compatibility entry at release index {idx}, entry index {compat_idx} must include string key: targetRelease"
                    )
                });

            assert!(
                release_name_to_index.contains_key(target_release),
                "compatibility entry at release index {idx}, entry index {compat_idx} references unknown targetRelease: {target_release}"
            );
        }
    }
}

#[test]
fn policy_and_interface_docs_exist_and_include_required_phrases() {
    let interface_path = workspace_path("docs/reference/public-interface-manifest.md");
    let deprecation_path = workspace_path("docs/reference/deprecation-policy.md");

    assert!(
        interface_path.exists(),
        "missing required artifact: {}",
        interface_path.display()
    );
    assert!(
        deprecation_path.exists(),
        "missing required artifact: {}",
        deprecation_path.display()
    );

    let interface_content = fs::read_to_string(&interface_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", interface_path.display()));
    let deprecation_content = fs::read_to_string(&deprecation_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", deprecation_path.display()));

    let combined_lower = format!("{interface_content}\n{deprecation_content}").to_lowercase();

    for phrase in ["stable", "beta", "experimental", "n+1", "n+2"] {
        assert!(
            combined_lower.contains(phrase),
            "required phrase not found in reference artifacts: {phrase}"
        );
    }

    let deprecation_lines: Vec<String> = deprecation_content
        .lines()
        .map(|line| line.trim().to_lowercase())
        .collect();

    let n_line = deprecation_lines
        .iter()
        .find(|line| line.starts_with("-") && line.contains("**n "))
        .expect("deprecation policy must include lifecycle bullet for N");
    assert!(
        n_line.contains("warn"),
        "deprecation lifecycle for N must mention warn"
    );

    let n_plus_1_line = deprecation_lines
        .iter()
        .find(|line| line.starts_with("-") && line.contains("**n+1"))
        .expect("deprecation policy must include lifecycle bullet for N+1");
    assert!(
        n_plus_1_line.contains("soft-fail"),
        "deprecation lifecycle for N+1 must mention soft-fail"
    );

    let n_plus_2_line = deprecation_lines
        .iter()
        .find(|line| line.starts_with("-") && line.contains("**n+2"))
        .expect("deprecation policy must include lifecycle bullet for N+2");
    assert!(
        n_plus_2_line.contains("remove"),
        "deprecation lifecycle for N+2 must mention remove"
    );

    for runbook in [
        "docs/runbooks/policy-timeout-storm.md",
        "docs/runbooks/degraded-git-backend.md",
    ] {
        let content = read_workspace_file(runbook);
        for stale_metric in [
            "claw_policy_eval_duration_seconds",
            "claw_policy_eval_total",
            "claw_retries_total",
            "claw_git_bridge_operation_duration_seconds",
            "claw_sync_queue_depth",
            "claw_sync_oldest_job_age_seconds",
        ] {
            assert!(
                !content.contains(stale_metric),
                "{runbook} must not reference unimplemented metric {stale_metric}"
            );
        }
    }
}

#[test]
fn adr_records_have_index_and_review_metadata() {
    let adr_index = read_workspace_file("docs/adr/README.md");
    for phrase in [
        "Architecture Decision Records",
        "ADR Template",
        "Supersession Policy",
        "0001",
        "0002",
        "0003",
        "0004",
        "0005",
        "0006",
        "0007",
    ] {
        assert!(adr_index.contains(phrase), "ADR index missing: {phrase}");
    }

    for adr in [
        "docs/adr/0001-no-staging-area.md",
        "docs/adr/0002-object-id-hashing.md",
        "docs/adr/0003-protobuf-and-cof.md",
        "docs/adr/0004-grpc-sync.md",
        "docs/adr/0005-intent-change-revision.md",
        "docs/adr/0006-capsules-as-repo-objects.md",
        "docs/adr/0007-policy-objects-in-repo.md",
    ] {
        let content = read_workspace_file(adr);
        for phrase in [
            "## Status",
            "## Metadata",
            "Date: 2026-05-12",
            "Owner: @Shree-git",
            "Supersedes:",
            "Superseded by:",
            "Related artifacts:",
            "## Alternatives Considered",
            "## Verification Links",
        ] {
            assert!(
                content.contains(phrase),
                "{adr} missing ADR metadata: {phrase}"
            );
        }
    }
}

#[test]
fn public_launch_assets_exist_and_are_upload_ready() {
    for artifact in [
        "scripts/demo.sh",
        "scripts/public-launch-preflight.sh",
        "scripts/verify-generated-protos.sh",
        "scripts/verify-github-labels.sh",
        "scripts/verify-name-clearance-evidence.sh",
        "scripts/publish-cratesio.sh",
        "scripts/verify-release-channel.sh",
        "examples/basic-demo/scripts/demo.sh",
        "docs/assets/social-preview.png",
        "docs/assets/social-preview.svg",
        "docs/operations/public-launch-checklist.md",
        "docs/operations/backlog-coverage.md",
        "docs/operations/external-blockers.json",
        "docs/operations/package-registry-strategy.md",
        "docs/operations/name-clearance.md",
        "docs/operations/name-clearance-evidence.template.md",
        ".github/workflows/large-repo-drill.yml",
        ".github/workflows/release-channel-smoke.yml",
    ] {
        let path = workspace_path(artifact);
        assert!(
            path.exists(),
            "missing required public-launch artifact: {}",
            path.display()
        );
    }

    let demo_wrapper = read_workspace_file("scripts/demo.sh");
    assert!(
        demo_wrapper.contains("examples/basic-demo/scripts/demo.sh"),
        "top-level demo wrapper must delegate to the maintained basic demo"
    );

    let label_manifest = read_workspace_file(".github/labels.yml");
    for label in [
        "good first issue",
        "help wanted",
        "docs",
        "security",
        "protocol",
        "git-interop",
        "needs-design",
        "known-limitation",
        "bug",
        "enhancement",
        "policy",
        "capsules",
    ] {
        assert!(
            label_manifest.contains(&format!("- name: {label}")),
            "label manifest must preserve backlog label: {label}"
        );
    }

    let codeowners = read_workspace_file(".github/CODEOWNERS");
    for owner_path in [
        "/deny.toml",
        "/rust-toolchain.toml",
        "/RELEASING.md",
        "/supply-chain/",
        "/fuzz/",
        "/tests/vectors/",
    ] {
        assert!(
            codeowners.contains(owner_path),
            "CODEOWNERS must route review for sensitive launch artifact: {owner_path}"
        );
    }

    let pr_template = read_workspace_file(".github/PULL_REQUEST_TEMPLATE.md");
    for phrase in [
        "Release, migration, and rollback",
        "Release note / changelog needed",
        "Install, packaging, or artifact verification changed",
        "Rollback plan or operator recovery note",
        "Security and supply chain",
        "Dependency, license, SBOM, or cargo-vet impact",
        "Public interface, policy, object format, or protocol changed",
    ] {
        assert!(
            pr_template.contains(phrase),
            "PR template must prompt for release/security evidence: {phrase}"
        );
    }

    let launch_preflight = read_workspace_file("scripts/public-launch-preflight.sh");
    for phrase in [
        "secret_scanning",
        "dependabot/alerts?state=open",
        "required_signatures",
        "https://crates.io/api/v1/crates/$crate_name",
        "claw-vcs-core",
        "ShreeGit/ClawVCS",
        "docs/assets/social-preview.png",
        "usesCustomOpenGraphImage",
        "CLAW_PREFLIGHT_REQUIRE_PAGES",
        "CLAW_PREFLIGHT_STRICT",
        "CLAW_PREFLIGHT_NAME_EVIDENCE",
        "CLAW_PREFLIGHT_CRATESIO_OWNER",
        "local ignored-junk hygiene",
        "scripts/verify-github-labels.sh",
        "scripts/verify-name-clearance-evidence.sh",
        "GitHub labels do not match .github/labels.yml",
        "crates.io owner verified for $crate_name",
        "social preview dimensions are 1280x640",
        "name-clearance-evidence.md",
        "completed name/domain/social/package evidence",
    ] {
        assert!(
            launch_preflight.contains(phrase),
            "public-launch preflight must include phrase: {phrase}"
        );
    }
    assert!(
        !launch_preflight.contains("mapfile"),
        "public-launch preflight must remain compatible with macOS system Bash"
    );

    let release_verifier = read_workspace_file("scripts/verify-release-channel.sh");
    for phrase in [
        "gh release download",
        "gh release view",
        "targetCommitish",
        "git ls-remote --tags",
        "claw-installer.sh",
        "cosign verify-blob",
        "gh attestation verify",
        "--source-ref \"refs/tags/${tag}\"",
        "--source-digest \"$tag_commit\"",
        "--signer-workflow \"${repo}/.github/workflows/release.yml\"",
        "--deny-self-hosted-runners",
        "CLAW_RELEASE_VERIFY_REPORT",
        "schemaVersion: 1",
        "checks: .",
        "claw-${tag}.sbom.spdx.json",
        "claw-${tag}.release-metadata.json",
        "verify_sha256_entry \"$sbom\"",
        "verify_sha256_entry \"$metadata\"",
        "verify_sbom_attestation",
        "--predicate-type \"https://spdx.dev/Document/v2.3\"",
        "--tag \"$tag\"",
        "CLAW_VERIFY_HOMEBREW",
    ] {
        assert!(
            release_verifier.contains(phrase),
            "release-channel verifier must include phrase: {phrase}"
        );
    }

    let cratesio_publisher = read_workspace_file("scripts/publish-cratesio.sh");
    for phrase in [
        "CLAW_CRATESIO_PUBLISH=1",
        "claw-vcs-core",
        "claw-vcs-store",
        "claw-vcs",
        "cargo publish -p \"$package\" --dry-run --locked --allow-dirty --registry crates-io",
        "cargo publish -p \"$package\" --locked --registry crates-io",
        "skipping dry-run for $package until registry dependencies are live",
        "cannot dry-run $package until registry dependencies are live",
        "refusing to publish without CLAW_CRATESIO_PUBLISH=1",
        "CLAW_CRATESIO_EXPECTED_OWNER",
        "CLAW_CRATESIO_RELEASE_TAG",
        "CLAW_CRATESIO_REPO_URL",
        "https://crates.io/api/v1/crates/$package/$workspace_version",
        "git describe --tags --exact-match HEAD",
        "git ls-remote --tags",
        "refusing to publish from a dirty working tree",
        ".users[]? | select(.login == $owner)",
        "crates.io owner verified for $package",
    ] {
        assert!(
            cratesio_publisher.contains(phrase),
            "crates.io publisher must include phrase: {phrase}"
        );
    }

    let workspace_manifest = read_workspace_file("Cargo.toml");
    for phrase in [
        "readme = \"README.md\"",
        "keywords = [\"vcs\", \"provenance\", \"ai-agents\", \"version-control\"]",
        "categories = [\"command-line-utilities\", \"development-tools\"]",
        "publish = [\"crates-io\"]",
    ] {
        assert!(
            workspace_manifest.contains(phrase),
            "workspace package metadata must include crates.io publishing field: {phrase}"
        );
    }
    let cli_manifest = read_workspace_file("crates/claw/Cargo.toml");
    for phrase in [
        "readme.workspace = true",
        "keywords.workspace = true",
        "categories.workspace = true",
        "publish.workspace = true",
    ] {
        assert!(
            cli_manifest.contains(phrase),
            "publishable crates must inherit workspace publishing metadata: {phrase}"
        );
    }

    let social_preview = fs::read(workspace_path("docs/assets/social-preview.png"))
        .expect("social preview PNG must be readable");
    assert!(
        social_preview.starts_with(b"\x89PNG\r\n\x1a\n"),
        "social preview asset must be a PNG file"
    );
    assert!(
        social_preview.len() >= 24,
        "social preview PNG must include an IHDR header"
    );
    let width = u32::from_be_bytes(
        social_preview[16..20]
            .try_into()
            .expect("PNG width bytes must be present"),
    );
    let height = u32::from_be_bytes(
        social_preview[20..24]
            .try_into()
            .expect("PNG height bytes must be present"),
    );
    assert_eq!(
        (width, height),
        (1280, 640),
        "social preview must match the documented 1280x640 GitHub card dimensions"
    );
    assert!(
        social_preview.len() < 1_000_000,
        "social preview PNG must stay under GitHub's 1 MB upload limit"
    );

    let launch_checklist = read_workspace_file("docs/operations/public-launch-checklist.md");
    let landing_page = read_workspace_file("docs/index.html");
    let readme = read_workspace_file("README.md");
    assert!(
        readme.contains(
            "Manage auth profiles and tokens for explicit remote URLs; hosted remotes are planned"
        ),
        "README command summary must not imply hosted remotes are currently live"
    );
    assert!(
        !readme.contains("releases/latest/download"),
        "README installer examples must not resolve to the historical latest release"
    );
    assert!(
        readme.contains("releases/download/<launch-tag>/"),
        "README release-channel examples must require an explicitly verified launch tag"
    );
    assert!(
        readme.contains("Until that tag is recorded in the install verification log"),
        "README must tell users to stay on source install until a launch-hardening tag is verified"
    );

    let package_strategy = read_workspace_file("docs/operations/package-registry-strategy.md");
    for legacy_status in [
        "| GitHub Releases | live |",
        "| Homebrew | live |",
        "| Windows MSI | live |",
        "| Shell installer | live |",
        "| PowerShell installer | live |",
    ] {
        assert!(
            !package_strategy.contains(legacy_status),
            "package registry strategy must not mark historical/unverified channels as launch-ready: {legacy_status}"
        );
    }
    assert!(
        package_strategy.contains("historical artifact live; launch verification pending"),
        "package registry strategy must distinguish existing artifacts from launch-ready verification"
    );

    let helm_values = read_workspace_file("crates/claw/deploy/helm/claw/values.yaml");
    assert!(
        !helm_values.contains("ghcr.io/shree-git/claw-vcs") && !helm_values.contains("tag: latest"),
        "Helm defaults must not point at an unpublished official OCI image or latest tag"
    );
    let terraform_variables = read_workspace_file("crates/claw/deploy/terraform/variables.tf");
    assert!(
        !terraform_variables.contains("default     = \"ghcr.io/shree-git/claw-vcs\"")
            && !terraform_variables.contains("default     = \"latest\""),
        "Terraform defaults must not point at an unpublished official OCI image or latest tag"
    );
    let deploy_validation = read_workspace_file(".github/workflows/deploy-validation.yml");
    assert!(
        !deploy_validation.contains("--set image.repository=ghcr.io/shree-git/claw-vcs"),
        "deploy validation must render against the local smoke image, not an unpublished official OCI image"
    );

    assert!(
        !landing_page.contains("href=\"getting-started/quickstart.md\"")
            && !landing_page.contains("href=\"security/threat-model.md\"")
            && !landing_page.contains("href=\"security/verifying-releases.md\""),
        "static landing page must not link to raw relative Markdown files when published by the Pages artifact workflow"
    );
    assert!(
        launch_checklist.contains("docs/assets/social-preview.png"),
        "launch checklist must name the upload-ready social preview asset"
    );
    assert!(
        launch_checklist.contains("Package-name checks"),
        "launch checklist must record package-name verification evidence"
    );
    assert!(
        launch_checklist.contains("scripts/public-launch-preflight.sh"),
        "launch checklist must point maintainers to the public-launch preflight"
    );

    let backlog_coverage = read_workspace_file("docs/operations/backlog-coverage.md");
    let external_blockers_raw = read_workspace_file("docs/operations/external-blockers.json");
    let external_blockers: Value = serde_json::from_str(&external_blockers_raw)
        .expect("external blockers manifest must parse as JSON");
    assert_eq!(
        external_blockers
            .get("schemaVersion")
            .and_then(Value::as_i64),
        Some(1),
        "external blockers manifest must declare schema version 1"
    );
    assert_eq!(
        external_blockers.get("repository").and_then(Value::as_str),
        Some("Shree-git/claw-vcs"),
        "external blockers manifest must name the public repository"
    );
    let blockers = external_blockers
        .get("blockers")
        .and_then(Value::as_array)
        .expect("external blockers manifest must include blockers array");
    let blocker_ids: HashSet<&str> = blockers
        .iter()
        .map(|blocker| {
            blocker
                .get("id")
                .and_then(Value::as_str)
                .expect("each external blocker must have an id")
        })
        .collect();
    let expected_blocker_ids: HashSet<&str> =
        ["release-channel-verification"].into_iter().collect();
    assert_eq!(
        blocker_ids, expected_blocker_ids,
        "external blockers manifest must preserve the remaining owner-side launch blocker set"
    );
    for blocker in blockers {
        let id = blocker
            .get("id")
            .and_then(Value::as_str)
            .expect("blocker id must be a string");
        for key in [
            "category",
            "ownerAction",
            "requiredBefore",
            "description",
            "verification",
        ] {
            assert!(
                blocker.get(key).is_some(),
                "external blocker {id} must include key: {key}"
            );
        }
        assert!(
            blocker
                .get("verification")
                .and_then(Value::as_array)
                .is_some_and(|commands| !commands.is_empty()),
            "external blocker {id} must include at least one verification command"
        );
    }
    let allowed_coverage_statuses: HashSet<&str> = [
        "Implemented",
        "Verified",
        "External pending",
        "Not applicable",
        "Implemented + external setting",
        "Implemented + external run state",
        "Implemented + external ingestion",
        "Implemented + audit backlog",
    ]
    .into_iter()
    .collect();
    let mut covered_backlog_items = HashSet::new();
    let mut external_pending_items = Vec::new();
    let mut not_applicable_items = Vec::new();
    for line in backlog_coverage.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 3 {
            continue;
        }
        let Ok(item) = cells[0].parse::<usize>() else {
            continue;
        };

        assert!(
            (1..=110).contains(&item),
            "backlog coverage has out-of-range item: {item}"
        );
        assert!(
            covered_backlog_items.insert(item),
            "backlog coverage item is duplicated: {item}"
        );
        assert!(
            allowed_coverage_statuses.contains(cells[1]),
            "backlog coverage item {item} has unexpected status: {}",
            cells[1]
        );
        assert!(
            !cells[2].is_empty(),
            "backlog coverage item {item} must include evidence"
        );
        match cells[1] {
            "External pending" => external_pending_items.push(item),
            "Not applicable" => not_applicable_items.push(item),
            _ => {}
        }
    }
    for item in 1..=110 {
        assert!(
            covered_backlog_items.contains(&item),
            "backlog coverage is missing item {item}"
        );
    }
    assert_eq!(
        covered_backlog_items.len(),
        110,
        "backlog coverage must include exactly the 110 backlog items"
    );
    assert_eq!(
        external_pending_items,
        vec![10],
        "only release-channel verification should remain external pending"
    );
    assert_eq!(
        not_applicable_items,
        vec![100],
        "only optional funding should remain not applicable"
    );
    for blocker in [
        "branch-protection review/signature requirements were restored",
        "hardened public release",
        "external-blockers.json",
    ] {
        assert!(
            backlog_coverage.contains(blocker),
            "backlog coverage must preserve external blocker: {blocker}"
        );
    }
    assert!(
        launch_checklist.contains("external-blockers.json"),
        "launch checklist must link to the structured external blocker manifest"
    );

    let name_clearance_template =
        read_workspace_file("docs/operations/name-clearance-evidence.template.md");
    for phrase in [
        "Domains checked/reserved:",
        "Social handles checked/reserved:",
        "crates.io packages reserved/published:",
        "GitHub social preview uploaded: no",
        "pending",
        "not complete",
        "Final decision:",
    ] {
        assert!(
            name_clearance_template.contains(phrase),
            "name-clearance evidence template must include phrase: {phrase}"
        );
    }
    assert!(
        backlog_coverage.contains("External pending"),
        "backlog coverage must preserve external-blocker status"
    );

    let release_channel_smoke = read_workspace_file(".github/workflows/release-channel-smoke.yml");
    assert!(
        release_channel_smoke.contains("cargo-install-git-smoke:"),
        "release-channel smoke workflow must include a cargo install from Git job"
    );
    assert!(
        release_channel_smoke.contains("provenance-release-smoke:"),
        "release-channel smoke workflow must include a provenance verifier job"
    );
    assert!(
        release_channel_smoke.contains("CLAW_SKIP_CARGO_INSTALL=1")
            && release_channel_smoke.contains("scripts/verify-release-channel.sh \"$RELEASE_TAG\""),
        "provenance release smoke must reuse the release-channel verifier"
    );
    assert!(
        release_channel_smoke.contains("CLAW_RELEASE_VERIFY_REPORT="),
        "release-channel smoke workflow must write structured verification reports"
    );
    assert!(
        release_channel_smoke.contains("actions/upload-artifact@"),
        "release-channel smoke workflow must upload durable verification evidence"
    );
    assert!(
        release_channel_smoke.contains("needs: release-metadata"),
        "cargo install from Git smoke must use the resolved release metadata"
    );
    assert!(
        release_channel_smoke.contains("--tag \"$RELEASE_TAG\""),
        "cargo install from Git smoke must install the exact release tag under validation"
    );

    let large_repo_drill = read_workspace_file(".github/workflows/large-repo-drill.yml");
    for phrase in [
        "large_repo_10k_file_snapshot_status_and_path_filter_drill",
        "--ignored --test-threads=1",
        "timeout-minutes: 30",
        "actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd",
        "dtolnay/rust-toolchain@29eef336d9b2848a0b548edc03f92a220660cdb8",
    ] {
        assert!(
            large_repo_drill.contains(phrase),
            "large-repo drill workflow must include phrase: {phrase}"
        );
    }

    let release_workflow = read_workspace_file(".github/workflows/release.yml");
    for phrase in [
        "Verify signed artifacts before release upload",
        "Write release metadata",
        "claw-${RELEASE_TAG}.release-metadata.json",
        "cargo metadata --format-version=1 --locked",
        "Attest release SBOM",
        "actions/attest-sbom@",
        "subject-path: artifacts/*",
        "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/axodotdev/cargo-dist/releases/download/v0.30.3/cargo-dist-installer.sh | sh",
        "irm https://github.com/axodotdev/cargo-dist/releases/download/v0.30.3/cargo-dist-installer.ps1 | iex",
        "sha256sum -c sha256.sum --ignore-missing",
        "jq -e '",
        "cosign verify-blob",
        "gh attestation verify \"$artifact\" --repo \"$GITHUB_REPOSITORY\" \\",
        "--source-digest \"$GITHUB_SHA\"",
        "--signer-workflow \"${GITHUB_REPOSITORY}/.github/workflows/release.yml\"",
        "--predicate-type \"https://spdx.dev/Document/v2.3\"",
    ] {
        assert!(
            release_workflow.contains(phrase),
            "release workflow must pre-verify artifact provenance before upload: {phrase}"
        );
    }
    let verify_artifacts_workflow = read_workspace_file(".github/workflows/verify-artifacts.yml");
    for phrase in [
        "Verify release metadata",
        "claw-${RELEASE_TAG}.release-metadata.json",
        "Verified SBOM attestations",
        "--predicate-type \"https://spdx.dev/Document/v2.3\"",
    ] {
        assert!(
            verify_artifacts_workflow.contains(phrase),
            "artifact verifier must validate release metadata and SBOM attestations: {phrase}"
        );
    }
    let upload_index = release_workflow
        .find("gh release upload \"${{ needs.plan.outputs.tag }}\" --clobber artifacts/*")
        .expect("release workflow must upload the complete signed artifact set");
    let publish_index = release_workflow
        .find("gh release edit \"${{ needs.plan.outputs.tag }}\" --draft=false")
        .expect("release workflow must publish the GitHub Release after upload");
    assert!(
        upload_index < publish_index,
        "existing draft releases must receive the signed artifact set before --draft=false promotion"
    );

    let install_log = read_workspace_file("docs/operations/install-verification-log.md");
    assert!(
        install_log.contains("--tag <launch-tag>"),
        "install verification log must require tag-specific cargo install verification"
    );
    assert!(
        install_log.contains("scripts/verify-release-channel.sh <launch-tag>"),
        "install verification log must point maintainers to the clean-host helper"
    );
    for phrase in [
        "Launch-Hardening Release Evidence Template",
        "Cosign signatures",
        "GitHub artifact attestations",
        "SPDX SBOM readability",
    ] {
        assert!(
            install_log.contains(phrase),
            "install verification log must preserve provenance evidence coverage: {phrase}"
        );
    }
}

#[test]
#[cfg(not(windows))]
fn release_helper_scripts_have_safe_cli_guards() {
    let root = workspace_root();

    let preflight_help = Command::new("bash")
        .arg("scripts/public-launch-preflight.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("run public-launch preflight help");
    assert!(
        preflight_help.status.success(),
        "public-launch preflight --help should exit successfully"
    );
    let preflight_help = String::from_utf8(preflight_help.stdout).expect("help is utf-8");
    assert!(preflight_help.contains("CLAW_PREFLIGHT_STRICT"));

    let labels_help = Command::new("bash")
        .arg("scripts/verify-github-labels.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("run GitHub label verifier help");
    assert!(
        labels_help.status.success(),
        "GitHub label verifier --help should exit successfully"
    );
    let labels_help = String::from_utf8(labels_help.stdout).expect("help is utf-8");
    assert!(labels_help.contains("CLAW_LABEL_REPO"));

    let evidence_help = Command::new("bash")
        .arg("scripts/verify-name-clearance-evidence.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("run name-clearance evidence verifier help");
    assert!(
        evidence_help.status.success(),
        "name-clearance evidence verifier --help should exit successfully"
    );
    let evidence_help = String::from_utf8(evidence_help.stdout).expect("help is utf-8");
    assert!(evidence_help.contains("name-clearance-evidence.md"));

    let generated_help = Command::new("bash")
        .arg("scripts/verify-generated-protos.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("run generated protobuf verifier help");
    assert!(
        generated_help.status.success(),
        "generated protobuf verifier --help should exit successfully"
    );
    let generated_help = String::from_utf8(generated_help.stdout).expect("help is utf-8");
    assert!(generated_help.contains("crates/claw-core/src/generated"));

    let evidence_path = std::env::temp_dir().join(format!(
        "claw-name-clearance-evidence-{}.md",
        std::process::id()
    ));
    fs::write(
        &evidence_path,
        r#"# Name Clearance Evidence

- Date: 2026-05-12
- Reviewer: Launch maintainer
- Trademark databases checked: USPTO, WIPO, EUIPO
- Similar marks and disposition: none relevant
- Domains checked/reserved: pending owner review
- Social handles checked/reserved: clawvcs reserved
- crates.io packages reserved/published: claw-vcs package set reserved
- GitHub social preview uploaded: yes
- Counsel review required: no
- Final decision: approve launch identity
"#,
    )
    .expect("write placeholder evidence fixture");
    let placeholder_evidence = Command::new("bash")
        .arg("scripts/verify-name-clearance-evidence.sh")
        .arg(&evidence_path)
        .current_dir(&root)
        .output()
        .expect("run name-clearance evidence verifier on placeholder fixture");
    assert_eq!(
        placeholder_evidence.status.code(),
        Some(1),
        "name-clearance evidence verifier must reject placeholder values"
    );
    let stderr = String::from_utf8(placeholder_evidence.stderr).expect("stderr is utf-8");
    assert!(
        stderr.contains("Domains checked/reserved"),
        "placeholder evidence rejection should name the bad field"
    );

    fs::write(
        &evidence_path,
        r#"# Name Clearance Evidence

- Date: May 12 2026
- Reviewer: Launch maintainer
- Trademark databases checked: USPTO, WIPO, EUIPO
- Similar marks and disposition: no conflicting developer-tool marks found
- Domains checked/reserved: clawvcs.dev reserved by maintainer account
- Social handles checked/reserved: clawvcs reserved on launch channels
- crates.io packages reserved/published: claw-vcs package set owned by maintainer
- GitHub social preview uploaded: yes
- Counsel review required: maybe
- Final decision: approved for Claw VCS public launch
"#,
    )
    .expect("write malformed evidence fixture");
    let malformed_evidence = Command::new("bash")
        .arg("scripts/verify-name-clearance-evidence.sh")
        .arg(&evidence_path)
        .current_dir(&root)
        .output()
        .expect("run name-clearance evidence verifier on malformed fixture");
    assert_eq!(
        malformed_evidence.status.code(),
        Some(1),
        "name-clearance evidence verifier must reject malformed date and counsel values"
    );
    let stderr = String::from_utf8(malformed_evidence.stderr).expect("stderr is utf-8");
    assert!(
        stderr.contains("Date must use YYYY-MM-DD format")
            && stderr.contains("Counsel review required must be yes or no"),
        "malformed evidence rejection should name date and counsel fields"
    );

    fs::write(
        &evidence_path,
        r#"# Name Clearance Evidence

- Date: 2026-05-12
- Reviewer: Launch maintainer
- Trademark databases checked: USPTO, EUIPO
- Similar marks and disposition: no conflicting developer-tool marks found
- Domains checked/reserved: clawvcs.dev reserved by maintainer account
- Social handles checked/reserved: clawvcs reserved on launch channels
- crates.io packages reserved/published: claw-vcs package set owned by maintainer
- GitHub social preview uploaded: yes
- Counsel review required: no
- Final decision: approved for Claw VCS public launch
"#,
    )
    .expect("write incomplete concrete evidence fixture");
    let incomplete_concrete_evidence = Command::new("bash")
        .arg("scripts/verify-name-clearance-evidence.sh")
        .arg(&evidence_path)
        .current_dir(&root)
        .output()
        .expect("run name-clearance evidence verifier on incomplete concrete fixture");
    assert_eq!(
        incomplete_concrete_evidence.status.code(),
        Some(1),
        "name-clearance evidence verifier must require named trademark databases and packages"
    );
    let stderr = String::from_utf8(incomplete_concrete_evidence.stderr).expect("stderr is utf-8");
    assert!(
        stderr.contains("Trademark databases checked must mention WIPO")
            && stderr.contains("crates.io packages reserved/published must mention claw-vcs-core"),
        "incomplete concrete evidence rejection should name missing database and package"
    );

    fs::write(
        &evidence_path,
        r#"# Name Clearance Evidence

- Date: 2026-05-12
- Reviewer: Launch maintainer
- Trademark databases checked: USPTO, WIPO, EUIPO
- Similar marks and disposition: no conflicting developer-tool marks found
- Domains checked/reserved: clawvcs.dev reserved by maintainer account
- Social handles checked/reserved: clawvcs reserved on launch channels
- crates.io packages reserved/published: claw-vcs, claw-vcs-core, claw-vcs-store, claw-vcs-patch, claw-vcs-merge, claw-vcs-crypto, claw-vcs-policy, claw-vcs-sync, claw-vcs-git owned by maintainer
- GitHub social preview uploaded: yes
- Counsel review required: no
- Final decision: approved for Claw VCS public launch
"#,
    )
    .expect("write completed evidence fixture");
    let completed_evidence = Command::new("bash")
        .arg("scripts/verify-name-clearance-evidence.sh")
        .arg(&evidence_path)
        .current_dir(&root)
        .output()
        .expect("run name-clearance evidence verifier on completed fixture");
    assert!(
        completed_evidence.status.success(),
        "name-clearance evidence verifier should accept completed fixture"
    );

    let docs_operations_dir = root.join("docs/operations");
    let completed_evidence_from_docs = Command::new("bash")
        .arg("../../scripts/verify-name-clearance-evidence.sh")
        .arg(&evidence_path)
        .current_dir(&docs_operations_dir)
        .output()
        .expect("run name-clearance evidence verifier from docs/operations");
    assert!(
        completed_evidence_from_docs.status.success(),
        "name-clearance evidence verifier should resolve repository paths outside repo root"
    );

    let preflight_help_from_docs = Command::new("bash")
        .arg("../../scripts/public-launch-preflight.sh")
        .arg("--help")
        .current_dir(&docs_operations_dir)
        .output()
        .expect("run public-launch preflight help from docs/operations");
    assert!(
        preflight_help_from_docs.status.success(),
        "public-launch preflight help should work outside repo root"
    );

    let labels_help_from_docs = Command::new("bash")
        .arg("../../scripts/verify-github-labels.sh")
        .arg("--help")
        .current_dir(&docs_operations_dir)
        .output()
        .expect("run GitHub label verifier help from docs/operations");
    assert!(
        labels_help_from_docs.status.success(),
        "GitHub label verifier help should work outside repo root"
    );
    let _ = fs::remove_file(&evidence_path);

    let verify_without_tag = Command::new("bash")
        .arg("scripts/verify-release-channel.sh")
        .current_dir(&root)
        .output()
        .expect("run release verifier without tag");
    assert_eq!(
        verify_without_tag.status.code(),
        Some(2),
        "release verifier must fail safely without a tag"
    );

    let publish_without_opt_in = Command::new("bash")
        .args([
            "scripts/publish-cratesio.sh",
            "--publish",
            "--package",
            "claw-vcs-core",
        ])
        .env_remove("CLAW_CRATESIO_PUBLISH")
        .current_dir(&root)
        .output()
        .expect("run crates.io publisher without opt-in");
    assert_eq!(
        publish_without_opt_in.status.code(),
        Some(2),
        "crates.io publisher must refuse real publishing without env opt-in"
    );
    let stderr = String::from_utf8(publish_without_opt_in.stderr).expect("stderr is utf-8");
    assert!(stderr.contains("refusing to publish without CLAW_CRATESIO_PUBLISH=1"));

    let publish_without_owner = Command::new("bash")
        .args([
            "scripts/publish-cratesio.sh",
            "--publish",
            "--package",
            "claw-vcs-core",
        ])
        .env("CLAW_CRATESIO_PUBLISH", "1")
        .env_remove("CLAW_CRATESIO_EXPECTED_OWNER")
        .current_dir(&root)
        .output()
        .expect("run crates.io publisher without expected owner");
    assert_eq!(
        publish_without_owner.status.code(),
        Some(2),
        "crates.io publisher must refuse real publishing without expected owner"
    );
    let stderr = String::from_utf8(publish_without_owner.stderr).expect("stderr is utf-8");
    assert!(stderr.contains("refusing to publish without CLAW_CRATESIO_EXPECTED_OWNER"));
}

#[test]
fn supply_chain_policy_metadata_is_parseable_and_intentional() {
    let audits_path = workspace_path("supply-chain/audits.toml");
    let config_path = workspace_path("supply-chain/config.toml");
    let imports_lock_path = workspace_path("supply-chain/imports.lock");
    let dependency_policy_path = workspace_path("docs/maintainers/dependency-policy.md");

    for path in [
        &audits_path,
        &config_path,
        &imports_lock_path,
        &dependency_policy_path,
    ] {
        assert!(
            path.exists(),
            "missing required supply-chain artifact: {}",
            path.display()
        );
    }

    let audits_raw = fs::read_to_string(&audits_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", audits_path.display()));
    let audits: toml::Value = toml::from_str(&audits_raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", audits_path.display()));
    assert!(
        audits
            .get("audits")
            .and_then(toml::Value::as_table)
            .is_some(),
        "supply-chain/audits.toml must define an [audits] table"
    );

    let config_raw = fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", config_path.display()));
    let config: toml::Value = toml::from_str(&config_raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", config_path.display()));

    assert_eq!(
        config
            .get("cargo-vet")
            .and_then(|value| value.get("version"))
            .and_then(toml::Value::as_str),
        Some("0.10"),
        "cargo-vet config must declare the expected metadata version"
    );
    assert_eq!(
        config
            .get("imports")
            .and_then(|value| value.get("mozilla"))
            .and_then(|value| value.get("url"))
            .and_then(toml::Value::as_str),
        Some("https://raw.githubusercontent.com/mozilla/supply-chain/main/audits.toml"),
        "cargo-vet config must import Mozilla's audit set"
    );

    let exemptions = config
        .get("exemptions")
        .and_then(toml::Value::as_table)
        .expect("cargo-vet config must include dependency exemptions");
    assert!(
        !exemptions.is_empty(),
        "cargo-vet exemptions must be explicit rather than implicit"
    );

    let allowed_criteria: HashSet<&str> = ["safe-to-deploy", "safe-to-run"].into_iter().collect();
    let mut exemption_count = 0usize;
    for (crate_name, entries) in exemptions {
        let entries = entries
            .as_array()
            .unwrap_or_else(|| panic!("exemption for {crate_name} must be an array"));
        assert!(
            !entries.is_empty(),
            "exemption list for {crate_name} must not be empty"
        );
        for (idx, entry) in entries.iter().enumerate() {
            exemption_count += 1;
            let entry = entry
                .as_table()
                .unwrap_or_else(|| panic!("exemption entry {crate_name}[{idx}] must be a table"));
            assert!(
                entry.get("version").and_then(toml::Value::as_str).is_some(),
                "exemption entry {crate_name}[{idx}] must include version"
            );
            let criteria = entry
                .get("criteria")
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| {
                    panic!("exemption entry {crate_name}[{idx}] must include criteria")
                });
            assert!(
                allowed_criteria.contains(criteria),
                "exemption entry {crate_name}[{idx}] has unexpected criteria: {criteria}"
            );
        }
    }
    assert!(
        exemption_count >= 10,
        "cargo-vet config should enumerate concrete exemptions, found {exemption_count}"
    );

    for sensitive_crate in ["x25519-dalek", "zeroize_derive"] {
        let entries = exemptions
            .get(sensitive_crate)
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| panic!("missing sensitive exemption: {sensitive_crate}"));
        assert!(
            entries.iter().any(|entry| entry
                .get("notes")
                .and_then(toml::Value::as_str)
                .is_some_and(|notes| notes.contains("Initial cargo-vet backlog exemption"))),
            "sensitive exemption {sensitive_crate} must explain why it remains an exemption"
        );
    }

    let imports_lock = fs::read_to_string(&imports_lock_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", imports_lock_path.display()));
    assert!(
        imports_lock.contains("mozilla"),
        "cargo-vet imports lock must include the Mozilla import"
    );

    let dependency_policy_raw = fs::read_to_string(&dependency_policy_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", dependency_policy_path.display()));
    let dependency_policy = dependency_policy_raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for phrase in [
        "cargo audit",
        "cargo deny check",
        "Dependency Review",
        "Dependabot",
        "SBOM",
        "cargo-vet",
        "replacing those exemptions with real audits",
    ] {
        assert!(
            dependency_policy.contains(phrase),
            "dependency policy must mention: {phrase}"
        );
    }
}

#[test]
fn packaged_proto_copies_match_workspace_proto_sources() {
    let canonical_root = workspace_path("proto/claw");
    for packaged_root in ["crates/claw-core/proto/claw", "crates/claw-sync/proto/claw"] {
        for entry in fs::read_dir(&canonical_root).expect("canonical proto directory exists") {
            let entry = entry.expect("proto directory entry is readable");
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let source = fs::read(entry.path())
                .unwrap_or_else(|err| panic!("failed to read proto source {file_name}: {err}"));
            let packaged_path = workspace_path(packaged_root).join(file_name.as_ref());
            let packaged = fs::read(&packaged_path).unwrap_or_else(|err| {
                panic!(
                    "failed to read packaged proto {}: {err}",
                    packaged_path.display()
                )
            });
            assert_eq!(
                packaged,
                source,
                "packaged proto {} must match proto/claw/{file_name}",
                packaged_path.display()
            );
        }
    }
}
