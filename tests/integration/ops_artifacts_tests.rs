use serde_json::Value;
use std::collections::HashSet;
use std::fs;
#[cfg(not(windows))]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Command;
#[cfg(not(windows))]
use std::sync::OnceLock;

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

#[cfg(not(windows))]
fn claw_binary_for_script_tests() -> &'static Path {
    static CLAW_BINARY: OnceLock<PathBuf> = OnceLock::new();
    CLAW_BINARY.get_or_init(|| {
        let root = workspace_root();
        let status = Command::new("cargo")
            .current_dir(&root)
            .args(["build", "-q", "-p", "claw-vcs", "--bin", "claw"])
            .status()
            .expect("build claw binary for script tests");
        assert!(status.success(), "cargo build claw failed with {status}");

        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        let binary = target_dir.join("debug").join("claw");
        assert!(binary.exists(), "missing claw binary: {}", binary.display());
        binary
    })
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
fn cli_json_schema_reference_uses_explicit_v1_envelopes() {
    let schemas = read_workspace_file("docs/reference/cli-json-schemas.md");
    assert!(
        !schemas.contains("implicit v1"),
        "CLI JSON schema reference must not describe undocumented implicit v1 rows"
    );
    assert!(
        schemas.contains("`req_<milliseconds>_<counter>`")
            && schemas.contains("\"request_id\":\"req_1779240000000_0\""),
        "CLI JSON schema reference must document the concrete diagnostic request_id format"
    );

    let mut v1_rows = 0usize;
    for (line_idx, line) in schemas.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("| `claw ") {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        assert!(
            cells.len() >= 4,
            "CLI JSON schema row at line {} must have table cells: {trimmed}",
            line_idx + 1
        );

        let command = cells[0];
        let schema_version = cells[1].trim_matches('`');
        let stable_fields = cells[2];

        if schema_version == "1" {
            v1_rows += 1;
            for field in ["`schema_version`", "`action`"] {
                assert!(
                    stable_fields.contains(field),
                    "v1 CLI JSON row {command} must document stable top-level field {field}"
                );
            }
        } else {
            assert_eq!(
                command, "`claw mcp serve`",
                "non-v1 CLI JSON row must be the MCP JSON-RPC surface"
            );
            assert_eq!(
                schema_version, "MCP JSON-RPC",
                "MCP row must name the JSON-RPC schema surface explicitly"
            );
        }
    }

    assert!(
        v1_rows >= 30,
        "CLI JSON schema reference must preserve broad command coverage; found only {v1_rows} v1 rows"
    );

    for (command, action) in [
        ("`claw git-export --json`", "`action` is `git-export`"),
        ("`claw git-import --json`", "`action` is `git-import`"),
        ("`claw git-roundtrip --json`", "`action` is `git-roundtrip`"),
    ] {
        let row = schemas
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("| {command} |")))
            .unwrap_or_else(|| panic!("CLI JSON schema reference missing row for {command}"));
        assert!(
            row.contains(action),
            "CLI JSON schema row for {command} must document exact stable action value {action}"
        );
    }
    let auth_row = schemas
        .lines()
        .find(|line| line.trim_start().starts_with("| `claw auth --json "))
        .expect("CLI JSON schema reference missing auth JSON row");
    for phrase in [
        "`token_present`",
        "`token_source`",
        "`refresh_token_present`",
        "`expires_at_unix`",
        "`profile_count`",
        "Token values and token prefixes are never emitted",
    ] {
        assert!(
            auth_row.contains(phrase),
            "auth CLI JSON schema row must document non-secret credential field: {phrase}"
        );
    }
    let roundtrip_row = schemas
        .lines()
        .find(|line| {
            line.trim_start()
                .starts_with("| `claw git-roundtrip --json` |")
        })
        .expect("CLI JSON schema reference missing git-roundtrip JSON row");
    for phrase in [
        "`verified`",
        "`exported_git_commit`",
        "`imported_revision`",
        "`checks`",
        "`source_revision_count`",
        "`imported_revision_count`",
    ] {
        assert!(
            roundtrip_row.contains(phrase),
            "git-roundtrip CLI JSON schema row must document verification field: {phrase}"
        );
    }

    let admin_row = schemas
        .lines()
        .find(|line| line.trim_start().starts_with("| `claw admin --json "))
        .expect("CLI JSON schema reference missing admin JSON row");
    for phrase in [
        "preflight/migrate/backup/rollback/support-bundle",
        "`dry_run`",
        "`applied`",
        "`diff`",
        "`written`",
        "`path`",
        "`request_id`",
        "`redaction_count`",
        "`redactions`",
    ] {
        assert!(
            admin_row.contains(phrase),
            "admin CLI JSON schema row must document automation field: {phrase}"
        );
    }

    let patch_row = schemas
        .lines()
        .find(|line| line.trim_start().starts_with("| `claw patch --json "))
        .expect("CLI JSON schema reference missing patch JSON row");
    for phrase in [
        "create/apply/show",
        "`applied`",
        "`file`",
        "`bytes_written`",
    ] {
        assert!(
            patch_row.contains(phrase),
            "patch CLI JSON schema row must document apply receipt field: {phrase}"
        );
    }

    let plugin_row = schemas
        .lines()
        .find(|line| {
            line.trim_start()
                .starts_with("| `claw plugin --json check`")
        })
        .expect("CLI JSON schema reference missing plugin check JSON row");
    for phrase in [
        "`plugin.check`",
        "`ok`",
        "`plugin`",
        "`protocol`",
        "`timeout_ms`",
        "`request_id`",
        "`jsonrpc`",
        "`method`",
        "`plugin.initialize`",
    ] {
        assert!(
            plugin_row.contains(phrase),
            "plugin CLI JSON schema row must document handshake receipt field: {phrase}"
        );
    }

    let sync_row = schemas
        .lines()
        .find(|line| line.trim_start().starts_with("| `claw sync "))
        .expect("CLI JSON schema reference missing sync JSON row");
    for phrase in [
        "push/pull/clone",
        "`remote_ref_found`",
        "`fetched_count`",
        "`target_available`",
        "`worktree`",
        "`installed_ref_count`",
        "`skipped_refs`",
        "`checkout`",
    ] {
        assert!(
            sync_row.contains(phrase),
            "sync CLI JSON schema row must document receipt field: {phrase}"
        );
    }

    let resolve_row = schemas
        .lines()
        .find(|line| line.trim_start().starts_with("| `claw resolve "))
        .expect("CLI JSON schema reference missing resolve JSON row");
    for phrase in [
        "list/mark/abort",
        "`resolve.list`",
        "`merge_in_progress`",
        "`conflict_count`",
        "`ready_count`",
        "`unresolved_count`",
        "`has_markers`",
        "`resolve.mark`",
        "`remaining_conflict_count`",
        "`resolve.abort`",
        "`restored_revision`",
    ] {
        assert!(
            resolve_row.contains(phrase),
            "resolve CLI JSON schema row must document conflict automation field: {phrase}"
        );
    }

    let repair_row = schemas
        .lines()
        .find(|line| line.trim_start().starts_with("| `claw repair "))
        .expect("CLI JSON schema reference missing repair JSON row");
    for phrase in [
        "plan/apply",
        "`planned_count`",
        "`applied_count`",
        "`post_summary`",
        "`remaining_issue_count`",
        "`remaining_repairable_count`",
        "`invalid_ref_namespace`",
    ] {
        assert!(
            repair_row.contains(phrase),
            "repair CLI JSON schema row must document repair automation field: {phrase}"
        );
    }

    let cli_readme = read_workspace_file("docs/cli/README.md");
    assert!(
        cli_readme.contains("admin migration")
            && cli_readme.contains("git export/import")
            && cli_readme.contains("claw repair --json apply --dry-run")
            && cli_readme.contains("claw repair apply --dry-run")
            && !cli_readme.contains("Other dry-run commands emit human-readable"),
        "CLI README dry-run guidance must reflect the current scriptable dry-run JSON surface"
    );
    let dry_run_section = cli_readme
        .split("## Dry Runs")
        .nth(1)
        .and_then(|section| section.split("## Onboarding After Init").next())
        .expect("CLI README must include bounded Dry Runs section");
    assert!(
        !dry_run_section.contains("support-bundle"),
        "support-bundle writes files and must not be documented as a dry-run command"
    );

    let public_manifest = read_workspace_file("docs/reference/public-interface-manifest.md");
    assert!(
        public_manifest.contains("Successful CLI JSON schemas")
            && public_manifest.contains("docs/reference/cli-json-schemas.md")
            && public_manifest.contains("should ignore additive fields"),
        "public interface manifest must classify successful CLI JSON schemas and their upgrade rule"
    );
    assert!(
        public_manifest.contains("Plugin protocol v1")
            && public_manifest.contains("docs/reference/plugin-protocol-v1.md")
            && public_manifest.contains("claw plugin --json check --plugin <path>")
            && public_manifest.contains("plugin.check")
            && public_manifest.contains("additive receipt fields"),
        "public interface manifest must classify plugin protocol and compliance receipt stability"
    );
    assert!(
        public_manifest.contains("Operational evidence reports")
            && public_manifest.contains("docs/reference/release-channel-report.md")
            && public_manifest.contains("docs/reference/repo-health-report.md")
            && public_manifest.contains("preserve failed reports")
            && public_manifest.contains("should ignore additive fields"),
        "public interface manifest must classify operational evidence reports and their upgrade rule"
    );
    let stability = read_workspace_file("docs/reference/stability.md");
    assert!(
        stability.contains("| Successful CLI JSON schemas | Experimental, versioned |"),
        "stability reference must include the successful CLI JSON schema surface"
    );
    assert!(
        stability.contains("| Operational evidence reports | Experimental, versioned |"),
        "stability reference must include the operational evidence report surface"
    );
    assert!(
        stability.contains("| Plugin protocol v1 | Experimental, versioned |"),
        "stability reference must include the plugin protocol surface"
    );
    let object_tiers = read_workspace_file("docs/reference/object-stability-tiers.md");
    assert!(
        object_tiers.contains("CLI JSON success envelopes")
            && object_tiers.contains("`schema_version`, `action`")
            && object_tiers.contains("Nested object fields inherit their object-field tiers"),
        "object stability tiers must describe the successful CLI JSON envelope tier"
    );
    assert!(
        object_tiers.contains("Operational evidence reports")
            && object_tiers.contains("`schema_version` or `schemaVersion`")
            && object_tiers.contains("release-channel and repository-health helpers")
            && object_tiers.contains("Release Channel Report")
            && object_tiers.contains("Repository Health Report"),
        "object stability tiers must describe the operational evidence report tier"
    );
    assert!(
        object_tiers.contains("Plugin protocol v1")
            && object_tiers.contains("JSON-RPC envelope")
            && object_tiers.contains("initialize handshake"),
        "object stability tiers must describe the plugin protocol tier"
    );

    let compatibility = read_workspace_file("docs/reference/compatibility.md");
    assert!(
        compatibility.contains("claw log --json")
            && compatibility.contains("bare-array output")
            && compatibility.contains("action: \"log\""),
        "compatibility notes must call out the log --json envelope migration"
    );
}

#[test]
fn terminology_glossary_covers_core_terms() {
    let docs_readme = read_workspace_file("docs/README.md");
    let concepts_index = read_workspace_file("docs/concepts/index.md");
    let glossary = read_workspace_file("docs/concepts/terminology.md");

    for (artifact, content, link) in [
        (
            "docs README",
            docs_readme.as_str(),
            "concepts/terminology.md",
        ),
        ("concepts index", concepts_index.as_str(), "terminology.md"),
    ] {
        assert!(
            content.contains(link),
            "{artifact} must link to the terminology glossary: {link}"
        );
    }

    for (term, definition_phrase, boundary_phrase) in [
        (
            "Intent",
            "The structured reason work exists",
            "A branch, ticket comment, implementation diff",
        ),
        (
            "Change",
            "One implementation attempt toward an intent",
            "The full project history",
        ),
        (
            "Revision",
            "A recorded repository state",
            "The command that captured the state",
        ),
        (
            "Snapshot",
            "The atomic capture operation",
            "A long-lived branch",
        ),
        (
            "Capsule",
            "A signed provenance envelope for one revision",
            "The revision itself",
        ),
        ("Evidence", "A named claim", "A policy rule"),
        (
            "Policy",
            "Versioned in-repository rules",
            "Evidence that a check passed",
        ),
        (
            "Workstream",
            "An ordered stack of related changes",
            "An intent, branch, merge queue",
        ),
    ] {
        assert!(
            glossary.contains(&format!("| {term} |")),
            "terminology glossary must include table row for {term}"
        );
        assert!(
            glossary.contains(definition_phrase),
            "terminology glossary must define {term} with phrase: {definition_phrase}"
        );
        assert!(
            glossary.contains(boundary_phrase),
            "terminology glossary must document what {term} must not mean: {boundary_phrase}"
        );
    }

    for phrase in [
        "Intent -> Change -> Revision -> Capsule -> Evidence",
        "Policy evaluates that path",
        "A snapshot records the atomic capture",
        "A workstream orders multiple changes",
    ] {
        assert!(
            glossary.contains(phrase),
            "terminology glossary must preserve relationship phrase: {phrase}"
        );
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
        "scripts/verify-repo-health.sh",
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
        "docs/reference/release-channel-report.md",
        "docs/reference/repo-health-report.md",
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
        "CLAW_PREFLIGHT_HEALTH_REPO",
        "CLAW_PREFLIGHT_HEALTH_REPORT",
        "CLAW_HEALTH_REPORT=\"$health_report\"",
        "scripts/verify-repo-health.sh",
        "repository health evidence verified",
        "doctor/repair JSON evidence",
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

    let repo_health = read_workspace_file("scripts/verify-repo-health.sh");
    for phrase in [
        "claw version --json",
        "claw doctor --json --strict",
        "claw repair --json plan",
        "CLAW_HEALTH_CLAW_BIN",
        "CLAW_HEALTH_REPORT",
        "version command exited",
        "version JSON must be schema_version 1 with action version",
        "summary.error_count expected 0",
        "repairable_count expected 0",
        "repo_health.verify",
        "\"generated_at_ms\": int(time.time() * 1000)",
        "\"repo_path\": repo_path",
        "\"claw_bin\": claw_bin",
        "\"checks\":",
        "\"command\": \"claw version --json\"",
        "\"command\": \"claw doctor --json --strict\"",
        "\"command\": \"claw repair --json plan\"",
        "\"version\": version if version is not None else version_error",
        "Wrote failed health report",
        "\"ok\": not failures",
    ] {
        assert!(
            repo_health.contains(phrase),
            "repo health verifier must include phrase: {phrase}"
        );
    }
    assert!(
        !repo_health.contains("mapfile"),
        "repo health verifier must remain compatible with macOS system Bash"
    );
    let repo_health_reference = read_workspace_file("docs/reference/repo-health-report.md");
    for phrase in [
        "repo_health.verify",
        "CLAW_HEALTH_REPORT=<path>",
        "CLAW_HEALTH_CLAW_BIN=<path>",
        "`generated_at_ms`",
        "`repo_path`",
        "`claw_bin`",
        "`checks.version.exit_code`",
        "`checks.doctor.exit_code`",
        "`checks.repair_plan.exit_code`",
        "`checks.version.exit_code == 0`",
        "`summary.error_count == 0`",
        "`repairable_count == 0`",
        "action: \"version\"",
    ] {
        assert!(
            repo_health_reference.contains(phrase),
            "repo health report reference must include phrase: {phrase}"
        );
    }

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
        "Optional JSON report path to write on pass/fail",
        "schemaVersion: 1",
        "action: \"release_channel.verify\"",
        "ok: ($exitStatus == \"0\")",
        "exitStatus: ($exitStatus | tonumber)",
        "failureCommand: (if $failureCommand == \"\" then null else $failureCommand end)",
        "failureLine: (if $failureLine == \"\" then null else ($failureLine | tonumber) end)",
        "trap 'capture_failure \"$BASH_COMMAND\" \"$LINENO\"' ERR",
        "checks: .",
        "trap finish EXIT",
        "report_written=1",
        "Wrote failed release-channel verification report",
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

    let release_channel_reference = read_workspace_file("docs/reference/release-channel-report.md");
    for phrase in [
        "Release Channel Report",
        "CLAW_RELEASE_VERIFY_REPORT=<path>",
        "`schemaVersion`",
        "`action`",
        "`release_channel.verify`",
        "`ok`",
        "`exitStatus`",
        "`failureCommand`",
        "`failureLine`",
        "`releaseTarget`",
        "`tagCommit`",
        "`checks[].channel`",
        "`checks[].status`",
        "`checks[].details`",
        "`ok == true`",
        "`exitStatus == 0`",
        "`action == \"release_channel.verify\"`",
        "`releaseTarget == tagCommit`",
        "Cosign, SLSA provenance, and SBOM attestation checks",
        "Failed reports are still evidence",
        "`failureCommand` and `failureLine`",
    ] {
        assert!(
            release_channel_reference.contains(phrase),
            "release channel report reference must include phrase: {phrase}"
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
    let docs_readme = read_workspace_file("docs/README.md");
    let landing_page = read_workspace_file("docs/index.html");
    let landing_page_md = read_workspace_file("docs/landing-page.md");
    let playground = read_workspace_file("docs/playground/index.html");
    let playground_readme = read_workspace_file("docs/playground/README.md");
    let mcp_reference = read_workspace_file("docs/reference/mcp-server.md");
    let mcp_cli = read_workspace_file("docs/cli/mcp.md");
    let rust_sdk = read_workspace_file("crates/claw-agent-sdk/src/lib.rs");
    let ts_sdk = read_workspace_file("sdk/typescript/src/index.ts");
    let py_sdk = read_workspace_file("sdk/python/src/claw_agent_sdk/__init__.py");
    let known_limitations = read_workspace_file("docs/reference/known-limitations.md");
    let readme = read_workspace_file("README.md");
    assert!(
        readme.contains(
            "Manage auth profiles and tokens for explicit remote URLs; hosted remotes are planned"
        ),
        "README command summary must not imply hosted remotes are currently live"
    );
    assert!(
        readme.contains("--auth-profile")
            && readme.contains("--auth-token-stdin")
            && readme.contains("process arguments"),
        "README daemon auth guidance must prefer saved profiles or stdin over process-argument secrets"
    );
    assert!(
        docs_readme.contains("Require bearer auth (`--auth-profile` or `--auth-token-stdin`)"),
        "docs README self-hosted baseline must prefer non-argument daemon auth sources"
    );
    assert!(
        docs_readme.contains("reference/release-channel-report.md")
            && docs_readme.contains("reference/repo-health-report.md"),
        "docs README must index machine-readable release and repository health report references"
    );
    let auth_cli = read_workspace_file("docs/cli/auth.md");
    assert!(
        auth_cli.contains("claw auth token set --stdin")
            && auth_cli.contains("claw auth --json token set --stdin"),
        "auth CLI docs must show stdin-based token import for human and JSON workflows"
    );
    for forbidden in [
        "claw auth token set <token>",
        "claw auth --json token set <token>",
        "claw auth token set \"<token>\"",
    ] {
        assert!(
            !auth_cli.contains(forbidden),
            "auth CLI docs must not lead with process-argument token examples: {forbidden}"
        );
    }
    let daemon_cli = read_workspace_file("docs/cli/daemon.md");
    assert!(
        daemon_cli.contains("--auth-profile")
            && daemon_cli.contains("--auth-token-stdin")
            && daemon_cli.contains("process arguments"),
        "daemon CLI docs must prefer saved profiles or stdin over process-argument secrets"
    );
    let troubleshooting = read_workspace_file("docs/operations/troubleshooting.md");
    assert!(
        troubleshooting.contains("claw auth token set --stdin")
            && !troubleshooting.contains("claw auth token set \"<token>\""),
        "troubleshooting auth recovery must avoid command-line token values"
    );
    let readiness = read_workspace_file("docs/reference/production-readiness-checklist.md");
    assert!(
        readiness.contains("`--auth-profile` or `--auth-token-stdin`")
            && !readiness.contains("`--auth-token` or `--auth-profile`"),
        "production readiness checklist must not recommend process-argument daemon tokens"
    );
    assert!(
        readiness.contains("Repository health")
            && readiness.contains("`claw doctor --json --strict`")
            && readiness.contains("`claw repair --json plan`")
            && readiness.contains("scripts/verify-repo-health.sh <repo>")
            && readiness.contains("CLAW_HEALTH_REPORT=<path>")
            && readiness.contains("both passing and failing gates")
            && readiness.contains("`generated_at_ms` timestamp")
            && readiness.contains("target `repo_path`, `claw_bin`")
            && readiness.contains("`claw_bin`")
            && readiness.contains("command exit")
            && readiness.contains("raw version/doctor/repair receipts")
            && readiness.contains("Repository Health Report")
            && readiness.contains("zero `summary.error_count`")
            && readiness.contains("zero `repairable_count`"),
        "production readiness checklist must gate on machine-readable doctor and repair health evidence"
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
    for codec_id in [
        "rust/ast",
        "typescript/ast",
        "python/ast",
        "sql/migration",
        "protobuf/ast",
        "terraform/tree",
        "toml/tree",
        "yaml/tree",
        "openapi/tree",
        "kubernetes/tree",
        "notebook/tree",
    ] {
        assert!(
            readme.contains(codec_id),
            "README must document semantic codec: {codec_id}"
        );
    }
    for (label, path) in [
        ("Beginner path", "persona/beginner.md"),
        ("Platform operator path", "persona/platform-operator.md"),
        ("Agent developer path", "persona/agent-developer.md"),
        ("Security reviewer path", "persona/security-reviewer.md"),
        ("VCS nerd cave", "persona/vcs-nerd-cave.md"),
    ] {
        assert!(
            docs_readme.contains(label) && docs_readme.contains(path),
            "docs README must expose persona route {label}: {path}"
        );
    }
    for route in [
        "docs/persona/beginner.md",
        "docs/persona/platform-operator.md",
        "docs/persona/agent-developer.md",
        "docs/persona/security-reviewer.md",
        "docs/persona/vcs-nerd-cave.md",
    ] {
        assert!(
            landing_page_md.contains(route),
            "landing page plan must route core persona: {route}"
        );
    }
    assert!(
        known_limitations.contains("Daemon-backed `claw sync pull` and `claw sync clone` expose partial-clone")
            && known_limitations.contains("Hosted `clawlab` HTTP remotes use the same filter request shape"),
        "known limitations must describe current partial-clone CLI support and hosted transport limits"
    );
    assert!(
        !known_limitations.contains("`claw sync clone` currently performs a full ref/object clone")
            && !known_limitations.contains("does not expose filter flags"),
        "known limitations must not preserve stale sync clone filter limitations"
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
        landing_page.contains("href=\"playground/\""),
        "static landing page must link to the browser playground"
    );
    for phrase in [
        "<title>Claw Playground</title>",
        "data-view-button=\"board\"",
        "id=\"intent-form\"",
        "id=\"change-form\"",
        "id=\"policy-form\"",
        "id=\"simulate-conflict\"",
        "id=\"export-json\"",
        "function graphRows()",
        "\"Goal\"",
        "\"Revision\"",
        "\"Capsule\"",
        "\"Blocker\"",
        "localStorage",
    ] {
        assert!(
            playground.contains(phrase),
            "playground must include interactive affordance: {phrase}"
        );
    }
    for phrase in [
        "intent creation",
        "agent-authored changes",
        "visual graph nodes for goals, revisions, capsules, evidence, policies, agents, and blockers",
        "policy dry-runs",
        "line-based conflict detection",
    ] {
        assert!(
            playground_readme.contains(phrase),
            "playground README must document capability: {phrase}"
        );
    }
    for phrase in [
        "claw mcp serve",
        "claw_status",
        "claw_intent_list",
        "claw_evidence_query",
        "claw_trust_receipt",
        "--allow-write",
    ] {
        assert!(
            mcp_reference.contains(phrase),
            "MCP reference must document capability: {phrase}"
        );
    }
    assert!(
        mcp_cli.contains("Model Context Protocol") && mcp_cli.contains("--allow-write"),
        "MCP CLI page must explain the MCP server and write opt-in"
    );
    assert!(
        rust_sdk.contains("pub struct ClawClient")
            && rust_sdk.contains("create_intent")
            && rust_sdk.contains("trust_receipt")
            && rust_sdk.contains("provenance_replay")
            && rust_sdk.contains("McpRequest"),
        "Rust agent SDK must expose typed agent workflow and MCP request helpers"
    );
    assert!(
        ts_sdk.contains("export class ClawClient")
            && ts_sdk.contains("trustReceipt")
            && ts_sdk.contains("provenanceReplay")
            && ts_sdk.contains("mcpServerCommand"),
        "TypeScript agent SDK must expose typed workflow and MCP launch helpers"
    );
    let ts_sdk_manifest = read_workspace_file("sdk/typescript/package.json");
    let ts_sdk_tsconfig = read_workspace_file("sdk/typescript/tsconfig.json");
    assert!(
        ts_sdk_manifest.contains("\"typecheck\": \"tsc --noEmit\"")
            && ts_sdk_manifest.contains("\"typescript\"")
            && ts_sdk_manifest.contains("\"@types/node\""),
        "TypeScript agent SDK package must declare a repeatable typecheck script and Node typings"
    );
    assert!(
        ts_sdk_tsconfig.contains("\"module\": \"NodeNext\"")
            && ts_sdk_tsconfig.contains("\"strict\": true")
            && ts_sdk_tsconfig.contains("\"types\": [\"node\"]"),
        "TypeScript agent SDK must include a strict NodeNext tsconfig"
    );
    assert!(
        py_sdk.contains("class ClawClient")
            && py_sdk.contains("def trust_receipt")
            && py_sdk.contains("def provenance_replay")
            && py_sdk.contains("def mcp_server_command"),
        "Python agent SDK must expose typed workflow and MCP launch helpers"
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
    assert!(
        launch_checklist.contains("CLAW_PREFLIGHT_HEALTH_REPO=<repo>")
            && launch_checklist.contains("CLAW_PREFLIGHT_HEALTH_REPORT=release-verification/repo-health.json")
            && launch_checklist.contains("CLAW_HEALTH_REPORT=release-verification/repo-health.json")
            && launch_checklist.contains("scripts/verify-repo-health.sh <repo>"),
        "launch checklist must show both preflight-integrated and standalone repository health evidence capture"
    );
    let release_checklist = read_workspace_file("docs/reference/release-checklist.md");
    assert!(
        release_checklist.contains("CLAW_PREFLIGHT_HEALTH_REPO=<repo>")
            && release_checklist.contains("CLAW_PREFLIGHT_HEALTH_REPORT=release-verification/repo-health.json")
            && release_checklist.contains("CLAW_HEALTH_REPORT=release-verification/repo-health.json")
            && release_checklist.contains("scripts/verify-repo-health.sh <repo>"),
        "release checklist must require repository health evidence capture when release environments already contain Claw repos"
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
        "pass/fail structured JSON report written to `CLAW_RELEASE_VERIFY_REPORT`",
        "writes pass/fail JSON evidence even when verification stops early",
        "Release Channel Report",
    ] {
        assert!(
            install_log.contains(phrase),
            "install verification log must preserve provenance evidence coverage: {phrase}"
        );
    }
    let release_reproducibility = read_workspace_file("docs/operations/release-reproducibility.md");
    let release_verification = read_workspace_file("docs/security/verifying-releases.md");
    for (name, content) in [
        ("release reproducibility", release_reproducibility),
        ("release verification", release_verification),
    ] {
        assert!(
            content.contains("reference/release-channel-report.md")
                || content.contains("../reference/release-channel-report.md"),
            "{name} docs must link to the release channel report schema"
        );
    }
    let release_checklist = read_workspace_file("docs/reference/release-checklist.md");
    assert!(
        release_checklist
            .contains("CLAW_RELEASE_VERIFY_REPORT=release-verification/<launch-tag>.json")
            && release_checklist.contains("produced a pass/fail")
            && release_checklist.contains("Preserve failed reports"),
        "release checklist must require preserving pass/fail release-channel verifier reports"
    );
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
    assert!(preflight_help.contains("CLAW_PREFLIGHT_HEALTH_REPO"));
    assert!(preflight_help.contains("CLAW_PREFLIGHT_HEALTH_REPORT"));

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

    let repo_health_help = Command::new("bash")
        .arg("scripts/verify-repo-health.sh")
        .arg("--help")
        .current_dir(&root)
        .output()
        .expect("run repo health verifier help");
    assert!(
        repo_health_help.status.success(),
        "repo health verifier --help should exit successfully"
    );
    let repo_health_help = String::from_utf8(repo_health_help.stdout).expect("help is utf-8");
    assert!(repo_health_help.contains("CLAW_HEALTH_REPORT"));

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

    let release_report_root = std::env::temp_dir().join(format!(
        "claw-release-report-failure-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&release_report_root);
    let fakebin = release_report_root.join("bin");
    fs::create_dir_all(&fakebin).expect("create fake release verifier bin dir");
    for (tool, body) in [
        ("gh", "#!/usr/bin/env bash\nexit 42\n"),
        ("cosign", "#!/usr/bin/env bash\nexit 0\n"),
        ("git", "#!/usr/bin/env bash\nexit 0\n"),
        ("tar", "#!/usr/bin/env bash\nexit 0\n"),
        ("shasum", "#!/usr/bin/env bash\nexit 0\n"),
    ] {
        let path = fakebin.join(tool);
        fs::write(&path, body).expect("write fake release verifier tool");
        let mut permissions = fs::metadata(&path)
            .expect("read fake tool metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("mark fake tool executable");
    }
    let failure_report = release_report_root.join("release-failed.json");
    let original_path = std::env::var("PATH").expect("PATH must be set for script tests");
    let release_failure = Command::new("bash")
        .arg("scripts/verify-release-channel.sh")
        .arg("v0.0.0-test")
        .env("PATH", format!("{}:{original_path}", fakebin.display()))
        .env("CLAW_RELEASE_VERIFY_REPORT", &failure_report)
        .current_dir(&root)
        .output()
        .expect("run release verifier with failing fake gh");
    assert_eq!(
        release_failure.status.code(),
        Some(42),
        "release verifier must preserve failing command exit status"
    );
    let release_failure_stderr =
        String::from_utf8(release_failure.stderr).expect("release failure stderr is utf-8");
    assert!(
        release_failure_stderr.contains("Wrote failed release-channel verification report"),
        "release verifier must announce failed report path"
    );
    let failure_json: Value =
        serde_json::from_str(&fs::read_to_string(&failure_report).expect("read failed report"))
            .expect("failed release report parses");
    assert_eq!(failure_json["schemaVersion"], 1);
    assert_eq!(failure_json["action"], "release_channel.verify");
    assert_eq!(failure_json["ok"], false);
    assert_eq!(failure_json["exitStatus"], 42);
    assert!(failure_json["failureCommand"]
        .as_str()
        .expect("failure command")
        .contains("gh release view"));
    assert!(failure_json["failureLine"].as_u64().is_some());
    assert_eq!(failure_json["tag"], "v0.0.0-test");
    assert!(failure_json["checks"]
        .as_array()
        .expect("checks array exists")
        .is_empty());
    fs::remove_dir_all(&release_report_root).expect("remove release report fixture");

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
#[cfg(not(windows))]
fn repo_health_verifier_writes_pass_and_fail_reports() {
    let root = workspace_root();
    let claw_bin = claw_binary_for_script_tests();
    let temp_root =
        std::env::temp_dir().join(format!("claw-repo-health-script-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(&temp_root).expect("create repo health temp root");

    let repo = temp_root.join("repo");
    let init = Command::new(claw_bin)
        .args(["init", repo.to_str().expect("temp repo path is utf-8")])
        .current_dir(&root)
        .output()
        .expect("initialize temp claw repo");
    assert!(
        init.status.success(),
        "claw init failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );

    let ok_report = temp_root.join("health-ok.json");
    let ok = Command::new("bash")
        .arg("scripts/verify-repo-health.sh")
        .arg(&repo)
        .env("CLAW_HEALTH_CLAW_BIN", claw_bin)
        .env("CLAW_HEALTH_REPORT", &ok_report)
        .current_dir(&root)
        .output()
        .expect("run repo health verifier on valid repo");
    assert!(
        ok.status.success(),
        "repo health verifier failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    let ok_json: Value =
        serde_json::from_str(&fs::read_to_string(&ok_report).expect("read passing health report"))
            .expect("passing health report parses");
    assert_eq!(ok_json["schema_version"], 1);
    assert_eq!(ok_json["action"], "repo_health.verify");
    assert_eq!(ok_json["ok"], true);
    assert!(ok_json["generated_at_ms"].as_u64().is_some());
    assert_eq!(ok_json["repo_path"], repo.to_str().expect("repo path"));
    assert_eq!(ok_json["version"]["action"], "version");
    assert_eq!(ok_json["doctor"]["action"], "doctor");
    assert_eq!(ok_json["repair_plan"]["action"], "repair.plan");
    assert_eq!(ok_json["checks"]["version"]["exit_code"], 0);
    assert_eq!(ok_json["checks"]["doctor"]["exit_code"], 0);
    assert_eq!(ok_json["checks"]["repair_plan"]["exit_code"], 0);

    let fail_report = temp_root.join("health-fail.json");
    let missing_repo = temp_root.join("missing-repo");
    let failed = Command::new("bash")
        .arg("scripts/verify-repo-health.sh")
        .arg(&missing_repo)
        .env("CLAW_HEALTH_CLAW_BIN", claw_bin)
        .env("CLAW_HEALTH_REPORT", &fail_report)
        .current_dir(&root)
        .output()
        .expect("run repo health verifier on missing repo");
    assert!(
        !failed.status.success(),
        "repo health verifier should fail for missing repo"
    );
    let fail_json: Value = serde_json::from_str(
        &fs::read_to_string(&fail_report).expect("read failing health report"),
    )
    .expect("failing health report parses");
    assert_eq!(fail_json["schema_version"], 1);
    assert_eq!(fail_json["action"], "repo_health.verify");
    assert_eq!(fail_json["ok"], false);
    assert_eq!(fail_json["version"]["action"], "version");
    assert_eq!(
        fail_json["repo_path"],
        missing_repo.to_str().expect("missing repo path")
    );
    assert!(fail_json["failures"]
        .as_array()
        .expect("failures array")
        .iter()
        .any(|failure| failure
            .as_str()
            .is_some_and(|value| value.contains("doctor command exited"))));
    assert!(
        fail_json["checks"]["doctor"]["exit_code"]
            .as_i64()
            .expect("doctor exit code")
            != 0
    );
    assert!(
        fail_json["checks"]["repair_plan"]["exit_code"]
            .as_i64()
            .expect("repair exit code")
            != 0
    );

    fs::remove_dir_all(&temp_root).expect("remove repo health temp root");
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
