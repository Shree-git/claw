mod support;

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use support::CliTestEnv;

fn run_git_ok(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("run git {:?}: {err}", args));
    assert!(
        output.status.success(),
        "git command failed in {}\n$ git {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn integrate_command_merges_feature_branch_and_updates_the_worktree() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("integrate-workflow");

    env.write_file(&repo.join("feature.txt"), "base\n");
    env.run_ok(&repo, ["snapshot", "-m", "Base revision"]);
    env.run_ok(&repo, ["branch", "create", "feature"]);
    env.run_ok(&repo, ["checkout", "feature"]);

    env.write_file(&repo.join("feature.txt"), "feature branch\n");
    env.run_ok(&repo, ["snapshot", "-m", "Feature revision"]);
    env.run_ok(&repo, ["checkout", "main"]);

    let integrated = env.run_ok(
        &repo,
        [
            "integrate",
            "--right",
            "heads/feature",
            "-m",
            "Merge feature",
        ],
    );
    assert!(integrated.stdout.contains("Integrated successfully"));
    assert_eq!(env.read_file(&repo.join("feature.txt")), "feature branch\n");

    let head = env.run_ok(&repo, ["show", "heads/main"]);
    assert!(head.stdout.contains("Merge feature"));
    assert!(head.stdout.contains("parents:"));

    let status = env.run_ok(&repo, ["status", "--json"]);
    assert_eq!(
        status.stdout_json()["changes"]
            .as_array()
            .expect("status changes array")
            .len(),
        0
    );
}

#[test]
fn admin_preflight_and_support_bundle_match_operator_docs() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("admin-operator-docs");

    let preflight = env.run_ok(&repo, ["admin", "preflight"]);
    assert!(preflight.stdout.contains("Preflight: PASS"));
    assert!(preflight.stdout.contains("metadata directory"));
    assert!(preflight.stdout.contains("tls configuration"));

    let preflight_json = env.run_ok(&repo, ["admin", "--json", "preflight"]);
    let preflight_json = preflight_json.stdout_json();
    assert_eq!(preflight_json["schema_version"], 1);
    assert_eq!(preflight_json["action"], "preflight");
    assert_eq!(preflight_json["ok"], true);
    assert!(
        preflight_json["summary"]["pass"]
            .as_u64()
            .unwrap_or_default()
            >= 1,
        "preflight JSON should include pass count"
    );
    assert_eq!(preflight_json["summary"]["fail"], 0);
    assert!(preflight_json["checks"]
        .as_array()
        .expect("preflight checks array")
        .iter()
        .any(|check| check["name"] == "metadata directory"
            && check["status"] == "pass"
            && check["next_step"].is_null()));

    std::fs::write(
        repo.join(".claw/config.toml"),
        r#"
config_version = 1

[tls]
cert_path = "/private/claw/client.pem"
key_path = "/private/claw/client-key.pem"
"#,
    )
    .expect("write config with sensitive TLS paths");

    let bundle_path = env.temp_root().join("support-bundle.json");
    let bundle = env.run_ok(
        &repo,
        [
            "admin",
            "support-bundle",
            "--out",
            bundle_path.to_str().expect("support bundle path utf-8"),
        ],
    );
    assert!(bundle.stdout.contains("Support bundle written:"));
    assert!(
        bundle_path.exists(),
        "support bundle file should be written"
    );

    let bundle_json: Value =
        serde_json::from_slice(&std::fs::read(&bundle_path).expect("read support bundle JSON"))
            .expect("support bundle must be valid JSON");
    assert_eq!(bundle_json["schema_version"], 1);
    assert_eq!(bundle_json["action"], "support-bundle");
    let expected_repo_root = std::fs::canonicalize(&repo).expect("canonicalize repo root");
    assert_eq!(
        bundle_json["repo_root"].as_str(),
        Some(expected_repo_root.to_str().expect("repo path utf-8"))
    );
    assert!(
        bundle_json["request_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("req_")),
        "support bundle must include a generated request id"
    );
    assert!(
        bundle_json["refs_count"].as_u64().is_some(),
        "support bundle must include refs_count"
    );
    assert_eq!(
        bundle_json["config"]["tls"]["cert_path"],
        "<redacted:support-bundle>"
    );
    assert_eq!(
        bundle_json["config"]["tls"]["key_path"],
        "<redacted:support-bundle>"
    );
    assert!(bundle_json["redactions"]
        .as_array()
        .expect("support bundle redactions")
        .iter()
        .any(|entry| entry == "config.tls.key_path"));

    let bundle_json_path = env.temp_root().join("support-bundle-json.json");
    let bundle_receipt = env.run_ok(
        &repo,
        [
            "admin",
            "--json",
            "support-bundle",
            "--out",
            bundle_json_path
                .to_str()
                .expect("support bundle JSON path utf-8"),
        ],
    );
    let bundle_receipt_json = bundle_receipt.stdout_json();
    assert_eq!(bundle_receipt_json["schema_version"], 1);
    assert_eq!(bundle_receipt_json["action"], "support-bundle");
    assert_eq!(bundle_receipt_json["written"], true);
    assert_eq!(
        bundle_receipt_json["path"].as_str(),
        Some(
            bundle_json_path
                .to_str()
                .expect("support bundle path utf-8")
        )
    );
    assert!(bundle_receipt_json["request_id"]
        .as_str()
        .is_some_and(|value| value.starts_with("req_")));
    assert_eq!(bundle_receipt_json["redaction_count"], 2);
    assert!(
        bundle_json_path.exists(),
        "support bundle JSON receipt should point to a written bundle file"
    );

    let ledger = std::fs::read_to_string(repo.join(".claw/migrations/ledger.jsonl"))
        .expect("support bundle should append admin ledger entry");
    assert!(ledger.contains("\"action\":\"support-bundle\""));
}

#[test]
fn admin_migrate_and_git_bridge_commands_work_end_to_end() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("git-bridge");

    run_git_ok(&repo, &["init", "-q"]);
    env.write_file(&repo.join("hello.txt"), "hello from claw\n");
    env.run_ok(&repo, ["snapshot", "-m", "Seed revision"]);

    let migration_plan = env.run_ok(&repo, ["admin", "migrate", "plan"]);
    assert!(migration_plan.stdout.contains("Migration plan ->"));
    assert!(migration_plan.stdout.contains(".claw/config.toml"));

    let migration_plan_json = env.run_ok(&repo, ["admin", "--json", "migrate", "plan"]);
    let migration_plan_json = migration_plan_json.stdout_json();
    assert_eq!(migration_plan_json["schema_version"], 1);
    assert_eq!(migration_plan_json["action"], "migrate.plan");
    assert_eq!(migration_plan_json["dry_run"], false);
    assert_eq!(migration_plan_json["applied"], false);
    assert!(migration_plan_json["backup_id"].is_null());
    assert!(migration_plan_json["target"]
        .as_str()
        .is_some_and(|target| target.ends_with(".claw/config.toml")));
    assert!(migration_plan_json["diff"]
        .as_str()
        .is_some_and(|diff| diff.contains(".claw/config.toml")));

    let dry_run = env.run_ok(&repo, ["admin", "migrate", "apply", "--dry-run"]);
    assert!(dry_run
        .stdout
        .contains("Dry run complete. No files changed."));

    let dry_run_json = env.run_ok(&repo, ["admin", "--json", "migrate", "apply", "--dry-run"]);
    let dry_run_json = dry_run_json.stdout_json();
    assert_eq!(dry_run_json["schema_version"], 1);
    assert_eq!(dry_run_json["action"], "migrate.apply");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["applied"], false);
    assert!(dry_run_json["backup_id"].is_null());

    let applied = env.run_ok(&repo, ["admin", "--json", "migrate", "apply"]);
    let applied_json = applied.stdout_json();
    assert_eq!(applied_json["schema_version"], 1);
    assert_eq!(applied_json["action"], "migrate.apply");
    assert_eq!(applied_json["dry_run"], false);
    assert_eq!(applied_json["applied"], true);
    assert!(applied_json["backup_id"]
        .as_str()
        .is_some_and(|backup_id| !backup_id.is_empty()));
    let ledger = std::fs::read_to_string(repo.join(".claw/migrations/ledger.jsonl"))
        .expect("migration ledger should be written");
    assert!(ledger.contains("\"action\":\"migrate.apply\""));

    let exported = env.run_ok(&repo, ["git-export"]);
    assert!(exported
        .stdout
        .contains("Exported to git: refs/heads/claw/main"));
    let exported_ref = run_git_ok(&repo, &["rev-parse", "--verify", "refs/heads/claw/main"]);
    assert_eq!(exported_ref.trim().len(), 40);

    let imported = env.run_ok(
        &repo,
        [
            "git-import",
            "--git-ref",
            "refs/heads/claw/main",
            "--ref-name",
            "heads/imported",
        ],
    );
    assert!(imported
        .stdout
        .contains("Imported git ref refs/heads/claw/main -> heads/imported"));

    let imported_ref = env.run_ok(&repo, ["show", "heads/imported"]);
    assert!(imported_ref.stdout.contains("Seed revision"));

    let roundtrip = env.run_ok(&repo, ["git-roundtrip", "--json"]);
    let roundtrip_json = roundtrip.stdout_json();
    assert_eq!(roundtrip_json["schema_version"], 1);
    assert_eq!(roundtrip_json["action"], "git-roundtrip");
    assert_eq!(roundtrip_json["verified"], true);
    assert_eq!(roundtrip_json["source_ref"], "heads/main");
    assert_eq!(
        roundtrip_json["exported_git_ref"],
        "refs/heads/claw/roundtrip-verify"
    );
    assert_eq!(roundtrip_json["import_ref"], "heads/roundtrip-verify");
    assert_eq!(roundtrip_json["with_notes"], false);
    assert_eq!(roundtrip_json["notes_imported"], serde_json::Value::Null);
    assert_eq!(roundtrip_json["checks"]["tree"], true);
    assert_eq!(roundtrip_json["checks"]["change_linkage"], true);
    assert_eq!(roundtrip_json["checks"]["ancestry"], true);
    assert!(roundtrip_json["source_revision"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));
    assert!(roundtrip_json["imported_revision"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));
    assert_eq!(
        roundtrip_json["checks"]["source_revision_count"],
        roundtrip_json["checks"]["imported_revision_count"]
    );
}
