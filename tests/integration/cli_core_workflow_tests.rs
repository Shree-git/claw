mod support;

use claw_core::{id::ObjectId, object::Object};
use claw_crypto::recipient::recipient_public_key;
use claw_store::ClawStore;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use support::CliTestEnv;

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn cli_dx_aliases_completions_and_init_hints_are_executable() {
    let env = CliTestEnv::new();
    let repo = env.repo_path("cli-dx-surfaces");
    std::fs::create_dir_all(&repo).expect("create repo dir");

    let init = env.run_ok(
        env.temp_root(),
        ["init", repo.to_str().expect("repo path utf-8")],
    );
    assert!(init.stdout.contains("Next steps:"));
    assert!(init.stdout.contains("claw status"));
    assert!(init
        .stdout
        .contains("claw snapshot -m \"initial snapshot\""));
    assert!(init.stdout.contains("claw intent create"));

    let json_repo = env.repo_path("init-json-surface");
    std::fs::create_dir_all(&json_repo).expect("create json init repo dir");
    let dry_init = env.run_ok(
        env.temp_root(),
        [
            "init",
            "--json",
            "--dry-run",
            json_repo.to_str().expect("json repo path utf-8"),
        ],
    );
    let dry_init_json = dry_init.stdout_json();
    assert_eq!(dry_init_json["schema_version"], 1);
    assert_eq!(dry_init_json["action"], "init");
    assert_eq!(dry_init_json["initialized"], false);
    assert_eq!(dry_init_json["created"], false);
    assert_eq!(dry_init_json["dry_run"], true);
    assert_eq!(dry_init_json["already_initialized"], false);
    assert_eq!(dry_init_json["head"], "heads/main");
    assert!(dry_init_json["path"]
        .as_str()
        .expect("init path")
        .ends_with("init-json-surface"));
    assert!(dry_init_json["claw_dir"]
        .as_str()
        .expect("init claw_dir")
        .ends_with("init-json-surface/.claw"));
    assert!(dry_init_json["next_steps"]
        .as_array()
        .expect("init next steps")
        .iter()
        .any(|step| step == "claw status"));
    assert!(
        !json_repo.join(".claw").exists(),
        "init --dry-run must not create .claw"
    );

    let json_init = env.run_ok(
        env.temp_root(),
        [
            "init",
            "--json",
            json_repo.to_str().expect("json repo path utf-8"),
        ],
    );
    let json_init = json_init.stdout_json();
    assert_eq!(json_init["schema_version"], 1);
    assert_eq!(json_init["action"], "init");
    assert_eq!(json_init["initialized"], true);
    assert_eq!(json_init["created"], true);
    assert_eq!(json_init["dry_run"], false);
    assert_eq!(json_init["already_initialized"], false);
    assert_eq!(json_init["head"], "heads/main");
    assert!(
        json_repo.join(".claw").exists(),
        "init --json must create .claw"
    );

    let completions = env.run_ok(env.temp_root(), ["completion", "bash"]);
    assert!(completions.stdout.contains("claw"));
    assert!(completions.stdout.contains("COMPREPLY"));

    let serve_help = env.run_ok(env.temp_root(), ["serve", "--help"]);
    assert!(serve_help.stdout.contains("Run the sync daemon"));

    let status_alias = env.run_ok(&repo, ["st", "--json"]);
    assert_eq!(status_alias.stdout_json()["branch"], "main");

    let branch_alias = env.run_ok(&repo, ["br"]);
    assert!(branch_alias.stdout.contains("* main (no commits yet)"));

    let branch_json = env.run_ok(&repo, ["branch", "--json"]);
    let branch_json = branch_json.stdout_json();
    assert_eq!(branch_json["schema_version"], 1);
    assert_eq!(branch_json["action"], "branch.list");
    assert_eq!(branch_json["current"], "main");
    assert_eq!(branch_json["branch_count"], 1);
    assert_eq!(branch_json["branches"][0]["name"], "main");
    assert_eq!(branch_json["branches"][0]["ref"], "heads/main");
    assert_eq!(branch_json["branches"][0]["current"], true);
    assert_eq!(
        branch_json["branches"][0]["target"],
        serde_json::Value::Null
    );
    assert_eq!(branch_json["branches"][0]["unborn"], true);

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Alias coverage",
            "--goal",
            "Exercise documented aliases",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["chg", "create", "--intent", intent_id.as_str()]);
    assert!(!change.value_after("Created change: ").is_empty());
}

#[test]
fn auth_token_json_receipts_are_scriptable_and_do_not_leak_secrets() {
    let env = CliTestEnv::new();

    let stored = env.run_ok_with_stdin(
        env.temp_root(),
        [
            "auth",
            "--json",
            "token",
            "set",
            "--stdin",
            "--profile",
            "prod",
            "--base-url",
            "https://daemon.example.invalid",
        ],
        "super-secret-token\n",
    );
    assert!(!stored.stdout.contains("super-secret-token"));
    let stored_json = stored.stdout_json();
    assert_eq!(stored_json["schema_version"], 1);
    assert_eq!(stored_json["action"], "auth.token.set");
    assert_eq!(stored_json["profile"], "prod");
    assert_eq!(stored_json["base_url"], "https://daemon.example.invalid");
    assert_eq!(stored_json["saved"], true);
    assert_eq!(stored_json["token_source"], "stdin");
    assert_eq!(stored_json["token_present"], true);

    let shown = env.run_ok(
        env.temp_root(),
        ["auth", "--json", "token", "show", "--profile", "prod"],
    );
    assert!(!shown.stdout.contains("super-secret-token"));
    let shown_json = shown.stdout_json();
    assert_eq!(shown_json["action"], "auth.token.show");
    assert_eq!(shown_json["profile"], "prod");
    assert_eq!(shown_json["token_present"], true);
    assert!(shown_json.get("access_token_masked").is_none());
    assert!(!shown.stdout.contains("super-secr"));

    let listed = env.run_ok(env.temp_root(), ["auth", "--json", "token", "list"]);
    assert!(!listed.stdout.contains("super-secret-token"));
    let listed_json = listed.stdout_json();
    assert_eq!(listed_json["action"], "auth.token.list");
    assert_eq!(listed_json["profile_count"], 1);
    assert_eq!(listed_json["profiles"][0]["profile"], "prod");
    assert_eq!(listed_json["profiles"][0]["token_present"], true);

    let logout = env.run_ok(
        env.temp_root(),
        ["auth", "--json", "logout", "--profile", "prod"],
    );
    let logout_json = logout.stdout_json();
    assert_eq!(logout_json["action"], "auth.logout");
    assert_eq!(logout_json["profile"], "prod");
    assert_eq!(logout_json["removed"], true);
}

#[test]
fn auth_config_parse_errors_are_reported_without_overwriting_tokens() {
    let env = CliTestEnv::new();
    let auth_path = env.temp_root().join("home").join(".claw").join("auth.toml");
    let broken_auth = "[profiles.prod\naccess_token = \"old-token\"\n";
    env.write_file(&auth_path, broken_auth);

    for args in [
        vec!["--error-format", "json", "auth", "--json", "token", "list"],
        vec![
            "--error-format",
            "json",
            "auth",
            "--json",
            "token",
            "show",
            "--profile",
            "prod",
        ],
        vec![
            "--error-format",
            "json",
            "auth",
            "--json",
            "token",
            "set",
            "new-token",
            "--profile",
            "prod",
            "--base-url",
            "https://daemon.example.invalid",
        ],
    ] {
        let result = env.run_fail(env.temp_root(), args);
        let error = result.stderr_json();
        assert_eq!(error["schema_version"], 1);
        assert_eq!(error["code"], "CONFIG_ERROR");
        assert_eq!(error["exit_code"], 4);
        assert!(error["message"]
            .as_str()
            .expect("error message")
            .contains("invalid auth config"));
        assert_eq!(
            env.read_file(&auth_path),
            broken_auth,
            "auth commands must not rewrite malformed auth.toml"
        );
    }
}

#[test]
fn patch_json_workbench_outputs_are_scriptable() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("patch-json-workbench");

    let codecs = env.run_ok(
        &repo,
        ["patch", "--json", "codecs", "--path", "api/openapi.yaml"],
    );
    let codecs_json = codecs.stdout_json();
    assert_eq!(codecs_json["schema_version"], 1);
    assert_eq!(codecs_json["action"], "patch.codecs");
    assert!(codecs_json["codec_count"].as_u64().unwrap_or(0) >= 12);
    assert_eq!(codecs_json["resolved"]["path"], "api/openapi.yaml");
    assert_eq!(codecs_json["resolved"]["codec"], "openapi/tree");
    let codec_ids = codecs_json["codecs"]
        .as_array()
        .expect("codec inventory")
        .iter()
        .filter_map(|codec| codec["id"].as_str())
        .collect::<Vec<_>>();
    assert!(codec_ids.contains(&"rust/ast"));
    assert!(codec_ids.contains(&"typescript/ast"));
    assert!(codec_ids.contains(&"python/ast"));
    assert!(codec_ids.contains(&"terraform/tree"));
    assert!(codec_ids.contains(&"notebook/tree"));
    let rust_codec = codecs_json["codecs"]
        .as_array()
        .expect("codec inventory")
        .iter()
        .find(|codec| codec["id"] == "rust/ast")
        .expect("rust codec inventory row");
    assert_eq!(rust_codec["operation_model"], "rust_top_level_ast_items");
    assert_eq!(rust_codec["canonical_output"], true);
    let sql_codec = codecs_json["codecs"]
        .as_array()
        .expect("codec inventory")
        .iter()
        .find(|codec| codec["id"] == "sql/migration")
        .expect("sql codec inventory row");
    assert_eq!(sql_codec["operation_model"], "sql_statements");
    let notebook_codec = codecs_json["codecs"]
        .as_array()
        .expect("codec inventory")
        .iter()
        .find(|codec| codec["id"] == "notebook/tree")
        .expect("notebook codec inventory row");
    assert_eq!(notebook_codec["operation_model"], "notebook_json_tree");

    env.write_file(&repo.join("old.txt"), "one\ntwo\n");
    env.write_file(&repo.join("new.txt"), "one\ntwo\nthree\n");
    env.write_file(&repo.join("apply.txt"), "one\ntwo\n");

    let created = env.run_ok(
        &repo,
        [
            "patch",
            "--json",
            "create",
            "--old",
            "old.txt",
            "--new",
            "new.txt",
            "--path",
            "notes.txt",
        ],
    );
    let created_json = created.stdout_json();
    assert_eq!(created_json["schema_version"], 1);
    assert_eq!(created_json["action"], "patch.create");
    assert_eq!(created_json["created"], true);
    assert_eq!(created_json["codec"], "text/line");
    assert_eq!(created_json["path"], "notes.txt");
    let patch_id = created_json["patch"]
        .as_str()
        .expect("patch id string")
        .to_string();
    assert!(patch_id.starts_with("clw_"));

    let shown = env.run_ok(&repo, ["patch", "--json", "show", patch_id.as_str()]);
    let shown_json = shown.stdout_json();
    assert_eq!(shown_json["schema_version"], 1);
    assert_eq!(shown_json["action"], "patch.show");
    assert_eq!(shown_json["patch"], patch_id);
    assert_eq!(shown_json["target_path"], "notes.txt");
    assert_eq!(shown_json["codec"], "text/line");
    assert_eq!(shown_json["ops"].as_array().map(Vec::len), Some(1));

    let inverted = env.run_ok(
        &repo,
        ["patch", "--json", "invert", "--patch", patch_id.as_str()],
    );
    let inverted_json = inverted.stdout_json();
    assert_eq!(inverted_json["schema_version"], 1);
    assert_eq!(inverted_json["action"], "patch.invert");
    assert_eq!(inverted_json["invertible"], true);
    assert_eq!(inverted_json["patch"]["id"], patch_id);
    assert_eq!(inverted_json["ops"].as_array().map(Vec::len), Some(1));

    let commute = env.run_ok(
        &repo,
        [
            "patch",
            "--json",
            "commute",
            "--left",
            patch_id.as_str(),
            "--right",
            patch_id.as_str(),
        ],
    );
    let commute_json = commute.stdout_json();
    assert_eq!(commute_json["schema_version"], 1);
    assert_eq!(commute_json["action"], "patch.commute");
    assert_eq!(commute_json["commutes"], true);
    assert!(commute_json["reordered"]["right_after_left"].is_array());

    let merged = env.run_ok(
        &repo,
        [
            "patch",
            "--json",
            "merge3",
            "--base",
            "old.txt",
            "--left",
            "new.txt",
            "--right",
            "old.txt",
            "--path",
            "notes.txt",
        ],
    );
    let merged_json = merged.stdout_json();
    assert_eq!(merged_json["schema_version"], 1);
    assert_eq!(merged_json["action"], "patch.merge3");
    assert_eq!(merged_json["merged"], true);
    assert_eq!(merged_json["codec"], "text/line");
    assert_eq!(merged_json["data_utf8"], "one\ntwo\nthree\n");

    let merged_out = env.run_ok(
        &repo,
        [
            "patch",
            "--json",
            "merge3",
            "--base",
            "old.txt",
            "--left",
            "new.txt",
            "--right",
            "old.txt",
            "--path",
            "notes.txt",
            "--out",
            "merged.txt",
        ],
    );
    let merged_out_json = merged_out.stdout_json();
    assert_eq!(merged_out_json["schema_version"], 1);
    assert_eq!(merged_out_json["action"], "patch.merge3");
    assert_eq!(merged_out_json["merged"], true);
    assert_eq!(merged_out_json["out"], "merged.txt");
    assert_eq!(merged_out_json["bytes"], 14);
    assert_eq!(env.read_file(&repo.join("merged.txt")), "one\ntwo\nthree\n");

    let workbench = env.run_ok(
        &repo,
        [
            "patch",
            "--json",
            "workbench",
            "--left",
            patch_id.as_str(),
            "--right",
            patch_id.as_str(),
        ],
    );
    let workbench_json = workbench.stdout_json();
    assert_eq!(workbench_json["schema_version"], 1);
    assert_eq!(workbench_json["action"], "patch.workbench");
    assert_eq!(workbench_json["workbench"], true);
    assert_eq!(workbench_json["classification"], "commutes");
    assert_eq!(workbench_json["commute"]["commutes"], true);
    assert!(workbench_json["commute"]["reordered"]["right_after_left"].is_array());
    assert_eq!(workbench_json["invert"]["left"]["invertible"], true);
    assert_eq!(workbench_json["invert"]["right"]["invertible"], true);
    assert_eq!(workbench_json["analysis"]["same_target_path"], true);
    assert_eq!(workbench_json["analysis"]["same_codec"], true);
    assert_eq!(workbench_json["analysis"]["same_base_object"], true);
    assert_eq!(workbench_json["analysis"]["same_result_object"], true);
    assert_eq!(workbench_json["analysis"]["left_op_count"], 1);
    assert_eq!(workbench_json["analysis"]["right_op_count"], 1);
    assert_eq!(workbench_json["analysis"]["reorder_available"], true);
    assert_eq!(workbench_json["analysis"]["left_invertible"], true);
    assert_eq!(workbench_json["analysis"]["right_invertible"], true);
    assert_eq!(workbench_json["analysis"]["decision"], "safe_to_reorder");
    assert!(workbench_json["analysis"]["left_addresses"].is_array());
    assert!(workbench_json["analysis"]["right_addresses"].is_array());
    assert!(workbench_json["analysis"]["overlapping_addresses"].is_array());
    assert!(workbench_json["why"]
        .as_array()
        .expect("workbench reasons")
        .iter()
        .any(|reason| reason.as_str().unwrap_or_default().contains("codec")));

    let applied = env.run_ok(
        &repo,
        [
            "patch",
            "--json",
            "apply",
            "--patch",
            patch_id.as_str(),
            "--file",
            "apply.txt",
        ],
    );
    let applied_json = applied.stdout_json();
    assert_eq!(applied_json["schema_version"], 1);
    assert_eq!(applied_json["action"], "patch.apply");
    assert_eq!(applied_json["applied"], true);
    assert_eq!(applied_json["patch"]["id"], patch_id);
    assert_eq!(applied_json["file"], "apply.txt");
    assert_eq!(applied_json["codec"], "text/line");
    assert_eq!(applied_json["bytes_written"], 14);
    assert_eq!(env.read_file(&repo.join("apply.txt")), "one\ntwo\nthree\n");
}

#[test]
fn integrate_merges_independent_rust_ast_items() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("integrate-rust-ast");
    std::fs::create_dir_all(repo.join("src")).expect("create src dir");

    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn alpha() -> &'static str {\n    \"base-alpha\"\n}\n\npub fn beta() -> &'static str {\n    \"base-beta\"\n}\n",
    );
    env.run_ok(&repo, ["snapshot", "-m", "Base Rust API"]);

    env.run_ok(&repo, ["branch", "create", "feature"]);
    env.run_ok(&repo, ["checkout", "feature"]);
    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn alpha() -> &'static str {\n    \"base-alpha\"\n}\n\npub fn beta() -> &'static str {\n    \"right-beta\"\n}\n",
    );
    env.run_ok(&repo, ["snapshot", "-m", "Feature edits beta"]);

    env.run_ok(&repo, ["checkout", "main"]);
    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn alpha() -> &'static str {\n    \"left-alpha\"\n}\n\npub fn beta() -> &'static str {\n    \"base-beta\"\n}\n",
    );
    env.run_ok(&repo, ["snapshot", "-m", "Main edits alpha"]);

    let preview = env.run_ok(
        &repo,
        [
            "integrate",
            "--json",
            "--right",
            "heads/feature",
            "--dry-run",
        ],
    );
    let preview_json = preview.stdout_json();
    assert_eq!(preview_json["schema_version"], 1);
    assert_eq!(preview_json["action"], "integrate");
    assert_eq!(preview_json["clean"], true);
    assert_eq!(preview_json["conflict_count"], 0);

    let integrated = env.run_ok(&repo, ["integrate", "--json", "--right", "heads/feature"]);
    let integrated_json = integrated.stdout_json();
    assert_eq!(integrated_json["schema_version"], 1);
    assert_eq!(integrated_json["action"], "integrate");
    assert_eq!(integrated_json["clean"], true);
    let result_revision = integrated_json["result_revision"]
        .as_str()
        .expect("result revision");

    let merged = env.read_file(&repo.join("src/lib.rs"));
    assert!(merged.contains("left-alpha"));
    assert!(merged.contains("right-beta"));

    let store = ClawStore::open(&repo).expect("open rust ast repo");
    let revision_id = ObjectId::from_display(result_revision).expect("parse result revision id");
    let revision = match store
        .load_object(&revision_id)
        .expect("load result revision")
    {
        Object::Revision(revision) => revision,
        other => panic!("expected revision, got {other:?}"),
    };
    let patch_codecs = revision
        .patches
        .iter()
        .map(
            |patch_id| match store.load_object(patch_id).expect("load integration patch") {
                Object::Patch(patch) => patch.codec_id,
                other => panic!("expected patch, got {other:?}"),
            },
        )
        .collect::<Vec<_>>();
    assert!(
        patch_codecs.iter().any(|codec| codec == "rust/ast"),
        "integration should retain rust/ast patches, got {patch_codecs:?}"
    );
}

#[test]
fn policy_lint_reports_dangerous_shapes_with_next_commands() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("policy-lint-ux");

    env.run_ok(&repo, ["policy", "create", "--id", "noop"]);
    env.run_ok(
        &repo,
        [
            "policy",
            "create",
            "--id",
            "fresh",
            "--require-fresh-evidence",
        ],
    );

    let human = env.run_ok(&repo, ["policy", "lint"]);
    assert!(human.stdout.contains("DANGER POLICY_NO_ENFORCEMENT"));
    assert!(human.stdout.contains("why:"));
    assert!(human
        .stdout
        .contains("try: claw policy apply --id noop --check test --dry-run"));
    assert!(human
        .stdout
        .contains("WARNING FRESHNESS_WITHOUT_REQUIRED_CHECKS"));
    assert!(human
        .stdout
        .contains("WARNING FRESHNESS_WITHOUT_TRUSTED_RUNNER"));

    let json = env.run_ok(&repo, ["policy", "lint", "fresh", "--json"]);
    let value = json.stdout_json();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["action"], "policy.lint");
    assert_eq!(value["policy_count"], 1);
    assert_eq!(value["policies"][0]["id"], "fresh");
    let codes = value["policies"][0]["findings"]
        .as_array()
        .expect("lint findings array")
        .iter()
        .map(|finding| finding["code"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"FRESHNESS_WITHOUT_REQUIRED_CHECKS"));
    assert!(codes.contains(&"FRESHNESS_WITHOUT_TRUSTED_RUNNER"));
}

#[test]
fn policy_simulator_explains_pass_and_fail_steps() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("policy-simulator-steps");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Policy simulation",
            "--goal",
            "Explain policy pass and fail paths",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(&repo.join("src").join("main.rs"), "fn main() {}\n");
    env.run_ok(
        &repo,
        [
            "snapshot",
            "-m",
            "Policy simulation target",
            "--change",
            change_id.as_str(),
        ],
    );

    env.run_ok(
        &repo,
        [
            "policy",
            "create",
            "--id",
            "bare-revision",
            "--check",
            "test",
        ],
    );
    let bare_denied = env.run_fail(
        &repo,
        [
            "policy",
            "simulate",
            "bare-revision",
            "--revision",
            "heads/main",
            "--json",
        ],
    );
    let bare_denied_json = bare_denied.stdout_json();
    assert_eq!(bare_denied_json["allowed"], false);
    assert!(bare_denied_json["capsule"]["id"].is_null());
    assert_eq!(bare_denied_json["capsule"]["source"], "synthetic_missing");
    assert_eq!(bare_denied_json["capsule"]["synthetic"], true);
    assert_eq!(
        bare_denied_json["simulation"]["first_failed_step"],
        "required_checks"
    );

    env.run_ok(
        &repo,
        [
            "ship",
            "--intent",
            intent_id.as_str(),
            "--evidence",
            "test=pass",
        ],
    );

    env.run_ok(
        &repo,
        ["policy", "create", "--id", "release", "--check", "test"],
    );
    let allowed = env.run_ok(
        &repo,
        [
            "policy",
            "simulate",
            "release",
            "--revision",
            "heads/main",
            "--json",
        ],
    );
    let allowed_json = allowed.stdout_json();
    assert_eq!(allowed_json["schema_version"], 1);
    assert_eq!(allowed_json["action"], "policy.eval");
    assert_eq!(allowed_json["allowed"], true);
    assert_eq!(allowed_json["simulation"]["failed_step_count"], 0);
    assert_eq!(
        allowed_json["simulation"]["passed_step_count"],
        allowed_json["simulation"]["step_count"]
    );
    assert_eq!(
        allowed_json["simulation"]["first_failed_step"],
        serde_json::Value::Null
    );
    let steps = allowed_json["simulation"]["steps"]
        .as_array()
        .expect("simulation steps");
    assert!(steps
        .iter()
        .any(|step| step["name"] == "required_checks" && step["passed"] == true));
    assert!(steps
        .iter()
        .any(|step| step["name"] == "external_plugins" && step["passed"] == true));

    env.run_ok(
        &repo,
        ["policy", "create", "--id", "strict", "--check", "lint"],
    );
    let denied = env.run_fail(
        &repo,
        [
            "policy",
            "simulate",
            "strict",
            "--revision",
            "heads/main",
            "--json",
        ],
    );
    let denied_json = denied.stdout_json();
    assert_eq!(denied_json["schema_version"], 1);
    assert_eq!(denied_json["action"], "policy.eval");
    assert_eq!(denied_json["allowed"], false);
    assert_eq!(
        denied_json["simulation"]["first_failed_step"],
        "required_checks"
    );
    assert!(denied_json["simulation"]["first_failure_reason"]
        .as_str()
        .expect("first failure reason")
        .contains("missing required check: lint"));
    assert!(
        denied_json["simulation"]["failed_step_count"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
    assert!(denied_json["simulation"]["failed_steps"]
        .as_array()
        .expect("failed steps")
        .iter()
        .any(|failure| failure["step"] == "required_checks"));
    let failed_required_check = denied_json["simulation"]["steps"]
        .as_array()
        .expect("denied simulation steps")
        .iter()
        .find(|step| step["name"] == "required_checks")
        .expect("required checks step");
    assert_eq!(failed_required_check["passed"], false);
    assert!(failed_required_check["reason"]
        .as_str()
        .expect("denial reason")
        .contains("missing required check: lint"));

    env.write_file(
        &repo.join("candidate-policy.json"),
        r#"{
  "policy_id": "candidate-file-policy",
  "required_checks": ["lint"],
  "required_reviewers": [],
  "sensitive_paths": [],
  "quarantine_lane": false,
  "visibility": "Public",
  "authorized_recipients": [],
  "revoked_recipients": [],
  "evidence_policy": {}
}
"#,
    );
    let file_denied = env.run_fail(
        &repo,
        [
            "policy",
            "simulate",
            "--policy-file",
            "candidate-policy.json",
            "--revision",
            "heads/main",
            "--json",
        ],
    );
    let file_denied_json = file_denied.stdout_json();
    assert_eq!(file_denied_json["policy"]["id"], "candidate-file-policy");
    assert_eq!(file_denied_json["policy"]["source"]["kind"], "file");
    assert_eq!(
        file_denied_json["policy"]["source"]["path"],
        "candidate-policy.json"
    );
    assert!(file_denied_json["policy"]["ref"].is_null());
    assert_eq!(file_denied_json["allowed"], false);

    env.run_ok(
        &repo,
        [
            "policy",
            "create",
            "--id",
            "sensitive-src",
            "--sensitive-path",
            "src/",
        ],
    );
    let sensitive_denied = env.run_fail(
        &repo,
        [
            "policy",
            "simulate",
            "sensitive-src",
            "--revision",
            "heads/main",
            "--json",
        ],
    );
    let sensitive_denied_json = sensitive_denied.stdout_json();
    assert_eq!(sensitive_denied_json["allowed"], false);
    assert_eq!(
        sensitive_denied_json["context"]["touched_paths_source"],
        "revision_patches"
    );
    assert!(sensitive_denied_json["context"]["touched_paths"]
        .as_array()
        .expect("derived touched paths")
        .contains(&serde_json::Value::String("src/main.rs".to_string())));
    assert_eq!(
        sensitive_denied_json["simulation"]["first_failed_step"],
        "sensitive_paths"
    );
    assert!(sensitive_denied_json["simulation"]["first_failure_reason"]
        .as_str()
        .expect("sensitive path failure reason")
        .contains("src/main.rs"));
}

#[test]
fn provenance_attach_attestation_dry_run_is_scriptable_and_non_mutating() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("provenance-attestation-dry-run");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Attach release evidence",
            "--goal",
            "Verify provenance dry-run output",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(&repo.join("artifact.txt"), "release artifact\n");
    env.run_ok(
        &repo,
        [
            "snapshot",
            "-m",
            "Artifact snapshot",
            "--change",
            change_id.as_str(),
        ],
    );
    let shipped = env.run_ok(
        &repo,
        [
            "ship",
            "--json",
            "--intent",
            intent_id.as_str(),
            "--evidence",
            "test=pass",
        ],
    );
    let shipped_json = shipped.stdout_json();
    assert_eq!(shipped_json["schema_version"], 1);
    assert_eq!(shipped_json["action"], "ship");
    let capsule_id = shipped_json["capsule_id"]
        .as_str()
        .expect("ship json capsule_id")
        .to_string();

    env.write_file(
        &repo.join("slsa.json"),
        r#"{
  "_type": "https://in-toto.io/Statement/v1",
  "predicateType": "https://slsa.dev/provenance/v1",
  "subject": [{
    "name": "artifact.txt",
    "digest": {"sha256": "abc123"}
  }],
  "predicate": {
    "builder": {"id": "https://github.com/actions/runner"},
    "buildType": "https://github.com/Actions"
  }
}
"#,
    );

    let attached = env.run_ok(
        &repo,
        [
            "provenance",
            "--json",
            "attach-attestation",
            "--revision",
            "heads/main",
            "--file",
            "slsa.json",
            "--subject-name",
            "artifact.txt",
            "--subject-digest",
            "sha256:abc123",
            "--builder-id",
            "https://github.com/actions/runner",
            "--build-type",
            "https://github.com/Actions",
            "--agent",
            "auditor",
            "--dry-run",
        ],
    );
    let attached_json = attached.stdout_json();
    assert_eq!(attached_json["schema_version"], 1);
    assert_eq!(attached_json["action"], "provenance.attach_attestation");
    assert_eq!(attached_json["attestation"]["status"], "pass");
    assert_eq!(
        attached_json["attestation"]["predicate_type"],
        "https://slsa.dev/provenance/v1"
    );
    assert_eq!(attached_json["attestation"]["subject_count"], 1);
    assert_eq!(
        attached_json["attestation"]["builder_id"],
        "https://github.com/actions/runner"
    );
    assert_eq!(
        attached_json["attestation"]["build_type"],
        "https://github.com/Actions"
    );
    assert_eq!(
        attached_json["attestation"]["validation"]["subject_matched"],
        true
    );
    assert_eq!(
        attached_json["attestation"]["validation"]["predicate_matched"],
        true
    );
    assert_eq!(attached_json["attachment"]["dry_run"], true);
    assert_eq!(attached_json["attachment"]["evidence_added"], 1);
    assert_eq!(
        attached_json["attachment"]["updated_capsule"],
        serde_json::Value::Null
    );
    let source_capsule = attached_json["attachment"]["source_capsule"]
        .as_str()
        .expect("source capsule hex");
    assert_eq!(source_capsule.len(), 64);
    assert!(source_capsule
        .chars()
        .all(|value| value.is_ascii_hexdigit()));

    let auditor = env.run_ok(&repo, ["agent", "status", "auditor", "--json"]);
    assert_eq!(auditor.stdout_json()["found"], false);

    let capsule_after = env.run_ok(&repo, ["log", "--json"]);
    let capsule_after_json = capsule_after.stdout_json();
    assert_eq!(capsule_after_json["schema_version"], 1);
    assert_eq!(capsule_after_json["action"], "log");
    let entries = capsule_after_json["entries"]
        .as_array()
        .expect("log entries");
    assert_eq!(entries[0]["capsule_id"].as_str(), Some(capsule_id.as_str()));
}

#[test]
fn provenance_replay_uses_filesystem_sandbox_by_default() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("provenance-replay-sandbox");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Replay sandbox evidence",
            "--goal",
            "Verify replay commands run outside repository metadata",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(&repo.join("replay.txt"), "sandbox input\n");
    env.run_ok(
        &repo,
        [
            "snapshot",
            "-m",
            "Replay sandbox target",
            "--change",
            change_id.as_str(),
        ],
    );
    let shipped = env.run_ok(
        &repo,
        [
            "ship",
            "--json",
            "--intent",
            intent_id.as_str(),
            "--evidence",
            "test=pass",
            "--evidence-command",
            "test ! -d .claw && test -f replay.txt",
        ],
    );
    let capsule_id = shipped.stdout_json()["capsule_id"]
        .as_str()
        .expect("ship capsule id")
        .to_string();

    let replay = env.run_ok(
        &repo,
        [
            "provenance",
            "--json",
            "replay",
            "--revision",
            "heads/main",
            "--capsule",
            capsule_id.as_str(),
            "--evidence",
            "test",
            "--dry-run",
        ],
    );
    let replay_json = replay.stdout_json();
    assert_eq!(replay_json["schema_version"], 1);
    assert_eq!(replay_json["action"], "provenance.replay");
    assert_eq!(replay_json["replay"]["workspace"], "sandbox");
    assert_eq!(replay_json["replay"]["sandboxed"], true);
    assert_eq!(replay_json["replay"]["selected_count"], 1);
    assert_eq!(replay_json["replay"]["replayed_count"], 1);
    assert_eq!(replay_json["replay"]["matched_count"], 1);
    assert_eq!(replay_json["replay"]["mismatched_count"], 0);
    assert_eq!(replay_json["replay"]["matched"], true);
    assert_eq!(replay_json["replay"]["results"][0]["workspace"], "sandbox");
    assert_eq!(replay_json["replay"]["results"][0]["matched"], true);
    assert!(replay_json["replay"]["results"][0]["started_at_ms"]
        .as_u64()
        .is_some());
    assert!(replay_json["replay"]["results"][0]["ended_at_ms"]
        .as_u64()
        .is_some());
    assert_eq!(replay_json["attachment"]["dry_run"], true);
    assert_eq!(
        replay_json["attachment"]["updated_capsule"],
        serde_json::Value::Null
    );

    let in_place = env.run_ok(
        &repo,
        [
            "provenance",
            "--json",
            "replay",
            "--revision",
            "heads/main",
            "--capsule",
            capsule_id.as_str(),
            "--evidence",
            "test",
            "--dry-run",
            "--in-place",
        ],
    );
    let in_place_json = in_place.stdout_json();
    assert_eq!(in_place_json["action"], "provenance.replay");
    assert_eq!(in_place_json["replay"]["workspace"], "repository");
    assert_eq!(in_place_json["replay"]["sandboxed"], false);
    assert_eq!(in_place_json["replay"]["matched_count"], 0);
    assert_eq!(in_place_json["replay"]["mismatched_count"], 1);
    assert_eq!(in_place_json["replay"]["matched"], false);
}

#[test]
fn core_cli_workflow_covers_init_snapshot_and_ship() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("core-workflow");

    let branch = env.run_ok(&repo, ["branch"]);
    assert!(branch.stdout.contains("* main (no commits yet)"));

    let status = env.run_ok(&repo, ["status", "--json"]);
    let json = status.stdout_json();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["action"], "status");
    assert_eq!(json["branch"], "main");
    assert!(json["head"].is_null());
    assert_eq!(json["in_merge"], false);
    assert_eq!(json["changes"].as_array().map(Vec::len), Some(0));

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "new",
            "--title",
            "Core workflow coverage",
            "--goal",
            "Exercise the local CLI path",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");

    let listed_intents = env.run_ok(&repo, ["intent", "list"]);
    assert!(listed_intents.stdout.contains(&intent_id));
    assert!(listed_intents.stdout.contains("Core workflow coverage"));

    let change = env.run_ok(&repo, ["change", "new", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    let listed_changes = env.run_ok(&repo, ["change", "list", "--intent", intent_id.as_str()]);
    assert!(listed_changes.stdout.contains(&change_id));

    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn greeting() -> &'static str {\n    \"hello from claw\"\n}\n",
    );

    let dirty_status = env.run_ok(&repo, ["status", "--json"]);
    let dirty_json = dirty_status.stdout_json();
    assert_eq!(dirty_json["schema_version"], 1);
    assert_eq!(dirty_json["action"], "status");
    assert!(dirty_json["head"].is_null());
    let changes = dirty_json["changes"]
        .as_array()
        .expect("status changes to be an array");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["path"], "src/lib.rs");
    assert_eq!(changes[0]["status"], "added");

    let snapshot = env.run_ok(
        &repo,
        [
            "snapshot",
            "--json",
            "-m",
            "Initial workflow snapshot",
            "--change",
            change_id.as_str(),
        ],
    );
    let snapshot_json = snapshot.stdout_json();
    assert_eq!(snapshot_json["schema_version"], 1);
    assert_eq!(snapshot_json["action"], "snapshot");
    assert_eq!(snapshot_json["snapshot_created"], true);
    assert_eq!(snapshot_json["merge_resolved"], false);
    assert_eq!(snapshot_json["branch"], "heads/main");
    assert_eq!(snapshot_json["patches"], 0);
    assert_eq!(snapshot_json["changed_files"], serde_json::Value::Null);
    let revision_display = snapshot_json["revision_id"]
        .as_str()
        .expect("snapshot revision id")
        .to_string();
    assert!(revision_display.starts_with("clw_"));

    let clean_snapshot = env.run_ok(&repo, ["snapshot", "--json", "-m", "No-op snapshot"]);
    let clean_snapshot_json = clean_snapshot.stdout_json();
    assert_eq!(clean_snapshot_json["schema_version"], 1);
    assert_eq!(clean_snapshot_json["action"], "snapshot");
    assert_eq!(clean_snapshot_json["snapshot_created"], false);
    assert_eq!(clean_snapshot_json["reason"], "clean");
    assert_eq!(clean_snapshot_json["branch"], "heads/main");
    assert_eq!(clean_snapshot_json["revision_id"], serde_json::Value::Null);
    assert_eq!(clean_snapshot_json["merge_resolved"], false);
    assert_eq!(clean_snapshot_json["patches"], 0);
    assert_eq!(clean_snapshot_json["changed_files"], 0);

    let clean_status = env.run_ok(&repo, ["status", "--json"]);
    let clean_status_json = clean_status.stdout_json();
    assert_eq!(clean_status_json["schema_version"], 1);
    assert_eq!(clean_status_json["action"], "status");
    assert_eq!(
        clean_status_json["head"].as_str(),
        Some(revision_display.as_str())
    );
    assert_eq!(
        clean_status_json["changes"].as_array().map(Vec::len),
        Some(0)
    );

    let show_head = env.run_ok(&repo, ["show", "heads/main"]);
    assert!(show_head.stdout.to_ascii_lowercase().contains("revision"));
    assert!(show_head.stdout.contains("change_id"));
    assert!(show_head.stdout.contains(&change_id));
    assert!(show_head.stdout.contains("Initial workflow snapshot"));

    let log_before_ship = env.run_ok(&repo, ["log", "--json"]);
    let log_before_ship_json = log_before_ship.stdout_json();
    assert_eq!(log_before_ship_json["schema_version"], 1);
    assert_eq!(log_before_ship_json["action"], "log");
    assert_eq!(log_before_ship_json["entry_count"], 1);
    let entries_before_ship = log_before_ship_json["entries"]
        .as_array()
        .expect("log entries before ship");
    assert_eq!(entries_before_ship.len(), 1);
    assert_eq!(
        entries_before_ship[0]["change_id"].as_str(),
        Some(change_id.as_str())
    );
    assert!(entries_before_ship[0].get("capsule_id").is_none());

    let recipient_secret = [9u8; 32];
    let recipient_public = recipient_public_key(&recipient_secret);
    env.write_file(
        &repo.join("capsule-private.json"),
        "{\"ticket\":\"SEC-1\",\"note\":\"reviewed\"}\n",
    );
    env.write_file(&repo.join("security.x25519"), &to_hex(&recipient_secret));

    let shipped = env.run_ok(
        &repo,
        [
            "ship",
            "--json",
            "--intent",
            intent_id.as_str(),
            "--evidence",
            "test=pass:42",
            "--evidence",
            "lint=pass",
            "--evidence-command",
            "cargo test --workspace",
            "--runner",
            "github-actions/release",
            "--environment-digest",
            "sha256:toolchain",
            "--log-digest",
            "sha256:log",
            "--evidence-expires-in-ms",
            "86400000",
            "--private-file",
            "capsule-private.json",
            "--recipient-key",
            &format!("security:security-key:{}", to_hex(&recipient_public)),
        ],
    );
    let shipped_json = shipped.stdout_json();
    assert_eq!(shipped_json["schema_version"], 1);
    assert_eq!(shipped_json["action"], "ship");
    assert_eq!(shipped_json["intent_id"], intent_id);
    assert_eq!(shipped_json["intent_status"], "Done");
    assert_eq!(shipped_json["change_id"], change_id);
    assert_eq!(shipped_json["revision_ref"], "heads/main");
    assert_eq!(shipped_json["agent_id"], "claw");
    assert_eq!(shipped_json["evidence_count"], 2);
    assert_eq!(shipped_json["signature_count"], 1);
    assert_eq!(shipped_json["private_fields_encrypted"], true);
    assert_eq!(shipped_json["recipient_count"], 1);
    let capsule_id = shipped_json["capsule_id"]
        .as_str()
        .expect("ship json capsule_id")
        .to_string();
    assert!(capsule_id.starts_with("clw_"));

    let show_capsule = env.run_ok(&repo, ["show", capsule_id.as_str()]);
    assert!(show_capsule.stdout.contains("agent_id"));
    assert!(show_capsule.stdout.contains("test (pass)"));
    assert!(show_capsule.stdout.contains("lint (pass)"));
    assert!(show_capsule.stdout.contains("private"));
    assert!(show_capsule.stdout.contains("security (security-key)"));

    let capsule_json = env.run_ok(&repo, ["show", "--json", capsule_id.as_str()]);
    let capsule_value = capsule_json.stdout_json();
    assert_eq!(capsule_value["schema_version"], 1);
    assert_eq!(capsule_value["action"], "show");
    assert_eq!(capsule_value["query"], capsule_id);
    assert_eq!(capsule_value["object_id"], capsule_id);
    assert_eq!(capsule_value["type"], "capsule");
    assert_eq!(capsule_value["object"]["id"], capsule_id);
    assert_eq!(capsule_value["object"]["hex"], capsule_value["object_hex"]);
    assert_eq!(capsule_value["object"]["type"], capsule_value["type"]);
    assert_eq!(capsule_value["object"]["value"], capsule_value["value"]);
    let capsule_object_hex = capsule_value["object_hex"]
        .as_str()
        .expect("capsule hex")
        .to_string();
    let evidence = &capsule_value["object"]["value"]["Capsule"]["public_fields"]["evidence"][0];
    assert_eq!(evidence["command"], "cargo test --workspace");
    assert_eq!(evidence["runner_identity"], "github-actions/release");
    assert_eq!(evidence["environment_digest"], "sha256:toolchain");
    assert_eq!(evidence["log_digest"], "sha256:log");

    let capsule_explorer = env.run_ok(&repo, ["capsule", "--json", "inspect", capsule_id.as_str()]);
    let capsule_explorer_json = capsule_explorer.stdout_json();
    assert_eq!(capsule_explorer_json["schema_version"], 1);
    assert_eq!(capsule_explorer_json["action"], "capsule.inspect");
    assert_eq!(capsule_explorer_json["agent_id"], "claw");
    assert_eq!(
        capsule_explorer_json["agent_identity"]["claimed_agent_id"],
        "claw"
    );
    assert_eq!(capsule_explorer_json["agent_identity"]["registered"], true);
    assert_eq!(capsule_explorer_json["agent_identity"]["status"], "active");
    assert_eq!(
        capsule_explorer_json["agent_identity"]["registered_identity_verified"],
        true
    );
    assert_eq!(
        capsule_explorer_json["agent_identity"]["verified_signature_count"],
        1
    );
    assert_eq!(
        capsule_explorer_json["agent_identity"]["registered_agent"]["agent_id"],
        "claw"
    );
    assert_eq!(
        capsule_explorer_json["execution_environment"]["environment_digests"][0],
        "sha256:toolchain"
    );
    assert_eq!(
        capsule_explorer_json["execution_environment"]["log_digests"][0],
        "sha256:log"
    );
    assert_eq!(
        capsule_explorer_json["execution_environment"]["runner_identities"][0],
        "github-actions/release"
    );
    assert_eq!(
        capsule_explorer_json["execution_environment"]["commands"][0],
        "cargo test --workspace"
    );
    assert_eq!(capsule_explorer_json["private_fields"]["present"], true);
    assert_eq!(capsule_explorer_json["private_fields"]["redacted"], true);
    assert_eq!(
        capsule_explorer_json["private_fields"]["recipients"][0]["recipient_id"],
        "security"
    );
    assert_eq!(
        capsule_explorer_json["trust_path"]["agent"]["registered"],
        true
    );
    assert_eq!(
        capsule_explorer_json["trust_path"]["agent"]["status"],
        "active"
    );
    assert_eq!(
        capsule_explorer_json["trust_path"]["cryptographic_integrity"],
        true
    );
    assert_eq!(
        capsule_explorer_json["trust_path"]["registered_identity_verified"],
        true
    );
    assert_eq!(capsule_explorer_json["signatures"][0]["verified"], true);
    assert_eq!(
        capsule_explorer_json["signatures"][0]["registered_agent"]["agent_id"],
        "claw"
    );

    let capsule_list = env.run_ok(&repo, ["capsule", "--json", "list"]);
    let capsule_list_json = capsule_list.stdout_json();
    assert_eq!(capsule_list_json["schema_version"], 1);
    assert_eq!(capsule_list_json["action"], "capsule.list");
    assert_eq!(capsule_list_json["count"], 1);
    assert_eq!(capsule_list_json["capsules"][0]["id"], capsule_id);

    let capsule_explorer_human = env.run_ok(&repo, ["capsule", "inspect", capsule_id.as_str()]);
    assert!(capsule_explorer_human
        .stdout
        .contains("registered_agent=true"));
    assert!(capsule_explorer_human
        .stdout
        .contains("registered_signature_verified=true"));

    let trust_receipt = env.run_ok(
        &repo,
        [
            "trust",
            "--json",
            "receipt",
            "--revision",
            "heads/main",
            "--capsule",
            capsule_id.as_str(),
        ],
    );
    let trust_receipt_json = trust_receipt.stdout_json();
    assert_eq!(trust_receipt_json["schema_version"], 1);
    assert_eq!(trust_receipt_json["action"], "trust.receipt");
    assert_eq!(trust_receipt_json["trustworthy"], true);
    assert_eq!(trust_receipt_json["summary"]["verdict"], "trustworthy");
    assert_eq!(trust_receipt_json["summary"]["policies_allowed"], true);
    assert_eq!(trust_receipt_json["summary"]["capsule_trustworthy"], true);
    assert_eq!(
        trust_receipt_json["summary"]["registered_identity_verified"],
        true
    );
    assert_eq!(trust_receipt_json["summary"]["verified_signature_count"], 1);
    assert_eq!(
        trust_receipt_json["capsule"]["trust_path"]["registered_identity_verified"],
        true
    );
    assert_eq!(
        trust_receipt_json["provenance"]["capsule_evidence_count"],
        2
    );
    assert_eq!(trust_receipt_json["provenance"]["policy_evidence_count"], 0);
    let receipt_evidence_names = trust_receipt_json["provenance"]["capsule_evidence"]
        .as_array()
        .expect("trust receipt capsule evidence")
        .iter()
        .filter_map(|evidence| evidence["name"].as_str())
        .collect::<Vec<_>>();
    assert!(receipt_evidence_names.contains(&"test"));
    assert!(receipt_evidence_names.contains(&"lint"));
    assert_eq!(
        trust_receipt_json["capsule"]["private_fields"]["recipient_count"],
        1
    );
    let why = trust_receipt_json["why"]
        .as_array()
        .expect("trust receipt why");
    assert!(why.iter().any(|reason| reason
        .as_str()
        .unwrap_or_default()
        .contains("registered active agent identity")));
    assert!(why.iter().any(|reason| reason
        .as_str()
        .unwrap_or_default()
        .contains("private capsule fields are encrypted")));

    let timeline_ref = env.run_ok(
        &repo,
        ["timeline", "--json", "ref", "--ref-name", "heads/main"],
    );
    let timeline_ref_json = timeline_ref.stdout_json();
    assert_eq!(timeline_ref_json["schema_version"], 1);
    assert_eq!(timeline_ref_json["action"], "timeline.ref");
    assert_eq!(timeline_ref_json["ref"], "heads/main");
    assert!(timeline_ref_json["entries"].as_array().is_some());

    let timeline_revision = env.run_ok(&repo, ["timeline", "--json", "revision", "heads/main"]);
    let timeline_revision_json = timeline_revision.stdout_json();
    assert_eq!(timeline_revision_json["schema_version"], 1);
    assert_eq!(timeline_revision_json["action"], "timeline.revision");
    assert_eq!(timeline_revision_json["revision"]["id"], revision_display);
    assert_eq!(
        timeline_revision_json["revision"]["capsule"]["id"],
        capsule_object_hex
    );

    let decrypted = env.run_ok(
        &repo,
        [
            "show",
            capsule_id.as_str(),
            "--decrypt-private",
            "--recipient",
            "security",
            "--recipient-secret-key",
            "security.x25519",
        ],
    );
    assert!(decrypted.stdout.contains("\"ticket\":\"SEC-1\""));

    env.run_ok(
        &repo,
        [
            "policy",
            "create",
            "--id",
            "revoked-recipient-policy",
            "--recipient",
            "security",
            "--revoked-recipient",
            "security",
        ],
    );
    let revoked = env.run_fail(
        &repo,
        [
            "policy",
            "eval",
            "revoked-recipient-policy",
            "--revision",
            "heads/main",
            "--capsule",
            capsule_id.as_str(),
            "--json",
        ],
    );
    assert!(revoked.combined_output().contains("revoked by policy"));

    let timeline_allowed = env.run_ok(
        &repo,
        [
            "timeline",
            "--json",
            "allowed",
            "--revision",
            "heads/main",
            "--policy",
            "revoked-recipient-policy",
        ],
    );
    let timeline_allowed_json = timeline_allowed.stdout_json();
    assert_eq!(timeline_allowed_json["schema_version"], 1);
    assert_eq!(timeline_allowed_json["action"], "timeline.allowed");
    assert_eq!(timeline_allowed_json["allowed"], false);
    assert_eq!(timeline_allowed_json["allowed_by"], 0);
    assert!(timeline_allowed_json["allowed_by_policy_ids"]
        .as_array()
        .expect("allowed policy ids")
        .is_empty());
    assert!(timeline_allowed_json["denied_by_policy_ids"]
        .as_array()
        .expect("denied policy ids")
        .contains(&serde_json::Value::String(
            "revoked-recipient-policy".to_string()
        )));
    assert!(timeline_allowed_json["first_seen_at_ms"].as_u64().is_some());
    assert_eq!(timeline_allowed_json["first_seen_ref"], "heads/main");
    assert!(timeline_allowed_json["first_allowed_at_ms"].is_null());
    assert!(timeline_allowed_json["first_allowed_by_policy_id"].is_null());
    assert_eq!(timeline_allowed_json["answer"]["allowed_now"], false);
    assert_eq!(
        timeline_allowed_json["answer"]["which_policy_let_this_through"],
        "none"
    );
    assert_eq!(
        timeline_allowed_json["answer"]["scope"],
        "current_policy_set"
    );
    assert!(
        timeline_allowed_json["answer"]["when_did_this_become_allowed"]
            .as_str()
            .expect("timeline answer text")
            .contains("no selected current policy allows it")
    );
    assert!(timeline_allowed_json["ref_events"]
        .as_array()
        .expect("timeline ref events")
        .iter()
        .any(|event| event["ref"] == "heads/main"));
    assert_eq!(
        timeline_allowed_json["policies"][0]["policy_id"],
        "revoked-recipient-policy"
    );
    assert_eq!(timeline_allowed_json["policies"][0]["allowed"], false);

    env.run_ok(&repo, ["policy", "create", "--id", "timeline-allow"]);
    let timeline_allowed_now = env.run_ok(
        &repo,
        [
            "timeline",
            "--json",
            "allowed",
            "--revision",
            "heads/main",
            "--policy",
            "timeline-allow",
        ],
    );
    let timeline_allowed_now_json = timeline_allowed_now.stdout_json();
    assert_eq!(timeline_allowed_now_json["schema_version"], 1);
    assert_eq!(timeline_allowed_now_json["action"], "timeline.allowed");
    assert_eq!(timeline_allowed_now_json["allowed"], true);
    assert_eq!(timeline_allowed_now_json["allowed_by"], 1);
    assert_eq!(
        timeline_allowed_now_json["first_allowed_by_policy_id"],
        "timeline-allow"
    );
    assert_eq!(
        timeline_allowed_now_json["first_allowed_policy_ref"],
        "policies/timeline-allow"
    );
    let revision_seen = timeline_allowed_now_json["first_seen_at_ms"]
        .as_u64()
        .expect("timeline allowed revision first seen");
    let first_allowed = timeline_allowed_now_json["first_allowed_at_ms"]
        .as_u64()
        .expect("timeline allowed first allowed");
    let policy_seen = timeline_allowed_now_json["policies"][0]["policy_first_seen_at_ms"]
        .as_u64()
        .expect("timeline policy first seen");
    assert!(first_allowed >= revision_seen);
    assert!(first_allowed >= policy_seen);
    assert_eq!(
        timeline_allowed_now_json["policies"][0]["allowed_since_ms"],
        first_allowed
    );
    assert_eq!(
        timeline_allowed_now_json["policies"][0]["allowed_since_basis"]
            ["revision_first_seen_at_ms"],
        revision_seen
    );
    assert_eq!(
        timeline_allowed_now_json["policies"][0]["allowed_since_basis"]["policy_first_seen_at_ms"],
        policy_seen
    );

    let evidence_query = env.run_ok(
        &repo,
        [
            "evidence",
            "--json",
            "query",
            "(test=pass OR lint=pass) AND signer.trust>=1.0",
            "--revision",
            "heads/main",
        ],
    );
    let evidence_query_json = evidence_query.stdout_json();
    assert_eq!(evidence_query_json["schema_version"], 1);
    assert_eq!(evidence_query_json["action"], "evidence.query");
    assert_eq!(
        evidence_query_json["query_plan"]["fields"],
        serde_json::json!(["evidence.name_status", "signer.trust"])
    );
    assert_eq!(
        evidence_query_json["query_plan"]["boolean_operators"],
        serde_json::json!(["AND", "OR"])
    );
    assert_eq!(evidence_query_json["query_plan"]["uses_signer_trust"], true);
    assert_eq!(
        evidence_query_json["query_plan"]["revision_filter"],
        "heads/main"
    );
    assert_eq!(evidence_query_json["count"], 2);
    assert!(evidence_query_json["matches"]
        .as_array()
        .expect("evidence query matches")
        .iter()
        .all(|item| item["agent_id"] == "claw"));
    assert!(evidence_query_json["matches"]
        .as_array()
        .expect("evidence query matches")
        .iter()
        .all(|item| item["matched_clauses"]
            .as_array()
            .expect("matched clauses")
            .iter()
            .any(|clause| clause["normalized_field"] == "signer.trust"
                && clause["actual"] == serde_json::json!(["1.000"]))));

    let intent_show = env.run_ok(&repo, ["intent", "show", intent_id.as_str()]);
    assert!(intent_show.stdout.contains("Status: Done"));

    let change_show = env.run_ok(&repo, ["change", "show", change_id.as_str()]);
    assert!(change_show.stdout.contains("Status: Integrated"));

    let log_after_ship = env.run_ok(&repo, ["log", "--json"]);
    let log_after_ship_json = log_after_ship.stdout_json();
    assert_eq!(log_after_ship_json["schema_version"], 1);
    assert_eq!(log_after_ship_json["action"], "log");
    assert_eq!(log_after_ship_json["entry_count"], 1);
    let entries_after_ship = log_after_ship_json["entries"]
        .as_array()
        .expect("log entries after ship");
    assert_eq!(entries_after_ship.len(), 1);
    assert_eq!(
        entries_after_ship[0]["capsule_id"].as_str(),
        Some(capsule_id.as_str())
    );

    let intent_graph = env.run_ok(&repo, ["intent", "--json", "graph"]);
    let intent_graph_json = intent_graph.stdout_json();
    assert_eq!(intent_graph_json["schema_version"], 1);
    assert_eq!(intent_graph_json["action"], "intent.graph");
    let graph_node_types = intent_graph_json["nodes"]
        .as_array()
        .expect("intent graph nodes")
        .iter()
        .filter_map(|node| node["type"].as_str())
        .collect::<Vec<_>>();
    for expected_type in [
        "intent", "change", "revision", "capsule", "evidence", "agent",
    ] {
        assert!(
            graph_node_types.contains(&expected_type),
            "intent graph missing node type {expected_type}: {graph_node_types:?}"
        );
    }
    assert!(intent_graph_json["edges"]
        .as_array()
        .expect("intent graph edges")
        .iter()
        .any(|edge| edge["relation"] == "claims"));

    let review = env.run_ok(&repo, ["review", "--json", "--intent", intent_id.as_str()]);
    let review_json = review.stdout_json();
    assert_eq!(review_json["schema_version"], 1);
    assert_eq!(review_json["action"], "review");
    assert_eq!(review_json["intent_count"], 1);
    assert_eq!(review_json["change_count"], 1);
    assert_eq!(review_json["capsule_count"], 1);
    assert_eq!(review_json["summary"]["review_required"], false);
    assert_eq!(review_json["summary"]["blocked_intent_count"], 0);
    assert_eq!(review_json["summary"]["missing_policy_count"], 0);
    assert_eq!(review_json["summary"]["missing_capsule_count"], 0);
    assert_eq!(review_json["summary"]["unsigned_capsule_count"], 0);
    assert_eq!(review_json["summary"]["total_evidence_count"], 2);
    assert_eq!(review_json["summary"]["failing_evidence_count"], 0);
    assert_eq!(review_json["filters"]["intent"], intent_id);
    assert_eq!(review_json["intents"][0]["id"], intent_id);
    assert_eq!(review_json["intents"][0]["changes"][0]["id"], change_id);
    let review_capsule = &review_json["intents"][0]["changes"][0]["head_revision"]["capsule"];
    assert_eq!(review_capsule["id"], capsule_id);
    let capsule_hex = review_capsule["hex"]
        .as_str()
        .expect("review capsule hex")
        .to_string();
    assert_eq!(review_json["index"]["changes"][0]["intent_id"], intent_id);
    assert_eq!(review_json["index"]["changes"][0]["change_id"], change_id);
    assert_eq!(review_json["index"]["changes"][0]["capsule"], capsule_hex);
    assert_eq!(review_json["index"]["changes"][0]["evidence_count"], 2);
    assert_eq!(review_json["index"]["capsules"][0]["intent_id"], intent_id);
    assert_eq!(review_json["index"]["capsules"][0]["change_id"], change_id);
    assert_eq!(review_json["index"]["capsules"][0]["capsule"], capsule_hex);
    assert_eq!(
        review_json["index"]["capsules"][0]["capsule_display"],
        capsule_id
    );

    let change_review = env.run_ok(&repo, ["review", "--json", "--change", change_id.as_str()]);
    let change_review_json = change_review.stdout_json();
    assert_eq!(change_review_json["schema_version"], 1);
    assert_eq!(change_review_json["action"], "review");
    assert_eq!(change_review_json["intent_count"], 1);
    assert_eq!(change_review_json["change_count"], 1);
    assert_eq!(change_review_json["filters"]["change"], change_id);
    assert_eq!(
        change_review_json["intents"][0]["changes"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let capsule_review = env.run_ok(
        &repo,
        ["review", "--json", "--capsule", capsule_id.as_str()],
    );
    let capsule_review_json = capsule_review.stdout_json();
    assert_eq!(capsule_review_json["schema_version"], 1);
    assert_eq!(capsule_review_json["action"], "review");
    assert_eq!(capsule_review_json["intent_count"], 1);
    assert_eq!(capsule_review_json["capsule_count"], 1);
    assert_eq!(capsule_review_json["filters"]["capsule"], capsule_hex);
    assert_eq!(
        capsule_review_json["intents"][0]["changes"][0]["head_revision"]["capsule"]["id"],
        capsule_id
    );

    let story = env.run_ok(
        &repo,
        [
            "story",
            "export",
            "--intent",
            intent_id.as_str(),
            "--format",
            "json",
        ],
    );
    let story_json = story.stdout_json();
    assert_eq!(story_json["schema_version"], 1);
    assert_eq!(story_json["action"], "story.export");
    assert_eq!(story_json["intent"]["id"], intent_id);
    assert_eq!(story_json["narrative"]["audit_verdict"], "ready_for_review");
    assert!(story_json["narrative"]["audience_summary"]
        .as_str()
        .expect("story audience summary")
        .contains("trust posture 'evidence_passed'"));
    assert_eq!(
        story_json["narrative"]["timeline"][0]["change_id"],
        change_id
    );
    assert_eq!(story_json["narrative"]["timeline"][0]["evidence_passed"], 2);
    assert_eq!(story_json["narrative"]["timeline"][0]["evidence_failed"], 0);
    assert_eq!(story_json["summary"]["signature_count"], 1);
    assert_eq!(story_json["summary"]["unsigned_capsule_count"], 0);
    assert_eq!(story_json["summary"]["private_capsule_count"], 1);
    assert_eq!(story_json["changes"][0]["id"], change_id);
    assert_eq!(
        story_json["changes"][0]["head_revision"]["capsule"]["id"],
        capsule_hex
    );
}

#[test]
fn ship_run_acceptance_attaches_runnable_specs_to_capsule_evidence() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("ship-acceptance-evidence");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Runnable acceptance",
            "--acceptance-test",
            "test -f src/lib.rs",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn accepted() -> bool { true }\n",
    );
    env.run_ok(
        &repo,
        [
            "snapshot",
            "--json",
            "-m",
            "acceptance target",
            "--change",
            change_id.as_str(),
        ],
    );

    let shipped = env.run_ok(
        &repo,
        [
            "ship",
            "--json",
            "--intent",
            intent_id.as_str(),
            "--run-acceptance",
        ],
    );
    let shipped_json = shipped.stdout_json();
    assert_eq!(shipped_json["schema_version"], 1);
    assert_eq!(shipped_json["action"], "ship");
    assert_eq!(shipped_json["evidence_count"], 1);
    assert_eq!(shipped_json["acceptance"]["run"], true);
    assert_eq!(shipped_json["acceptance"]["passed"], true);
    assert_eq!(shipped_json["acceptance"]["count"], 1);
    assert_eq!(
        shipped_json["acceptance"]["evidence_names"][0],
        "acceptance/1"
    );
    let capsule_id = shipped_json["capsule_id"]
        .as_str()
        .expect("ship capsule id");

    let capsule = env.run_ok(&repo, ["capsule", "--json", "inspect", capsule_id]);
    let capsule_json = capsule.stdout_json();
    let evidence = capsule_json["evidence"]
        .as_array()
        .expect("capsule evidence");
    let acceptance = evidence
        .iter()
        .find(|entry| entry["name"] == "acceptance/1")
        .expect("acceptance evidence");
    assert_eq!(acceptance["status"], "pass");
    assert_eq!(acceptance["command"], "test -f src/lib.rs");
    assert_eq!(acceptance["exit_code"], 0);
    assert_eq!(acceptance["trust_domain"], "acceptance");
    assert!(!acceptance["revision_id"].is_null());
    assert!(acceptance["started_at_ms"].as_u64().is_some());
    assert!(acceptance["ended_at_ms"].as_u64().is_some());

    let trust = env.run_ok(
        &repo,
        [
            "trust",
            "--json",
            "receipt",
            "--revision",
            "heads/main",
            "--capsule",
            capsule_id,
        ],
    );
    let trust_json = trust.stdout_json();
    assert_eq!(trust_json["provenance"]["capsule_evidence_count"], 1);
    let evidence_names = trust_json["provenance"]["capsule_evidence"]
        .as_array()
        .expect("trust evidence")
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect::<Vec<_>>();
    assert!(evidence_names.contains(&"acceptance/1"));
}

#[test]
fn ship_run_acceptance_failure_blocks_capsule_write() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("ship-acceptance-failure");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Failing acceptance",
            "--acceptance-test",
            "exit 7",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn rejected() -> bool { false }\n",
    );
    env.run_ok(
        &repo,
        [
            "snapshot",
            "--json",
            "-m",
            "rejected target",
            "--change",
            change_id.as_str(),
        ],
    );

    let failed = env.run_fail(
        &repo,
        [
            "ship",
            "--json",
            "--intent",
            intent_id.as_str(),
            "--run-acceptance",
        ],
    );
    assert!(failed.combined_output().contains("acceptance test"));
    assert!(failed.combined_output().contains("capsule was not written"));

    let capsules = env.run_ok(&repo, ["capsule", "--json", "list"]);
    let capsules_json = capsules.stdout_json();
    assert_eq!(capsules_json["count"], 0);
}

#[test]
fn agent_revoke_blocks_ship_until_rotation() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("agent-revoke-rotate");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Agent lifecycle",
            "--goal",
            "Exercise explicit agent key revocation",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(&repo.join("README.md"), "agent lifecycle\n");
    env.run_ok(
        &repo,
        [
            "snapshot",
            "-m",
            "tracked revision",
            "--change",
            change_id.as_str(),
        ],
    );

    env.run_ok(&repo, ["agent", "register", "--name", "ci-agent"]);

    let dry_revoke = env.run_ok(
        &repo,
        [
            "agent",
            "revoke",
            "--name",
            "ci-agent",
            "--reason",
            "compromised",
            "--dry-run",
        ],
    );
    assert!(dry_revoke.stdout.contains("Dry run: would revoke agent"));

    let active = env.run_ok(&repo, ["agent", "status", "ci-agent"]);
    assert!(active.stdout.contains("Status: active"));

    let dry_quarantine = env.run_ok(
        &repo,
        [
            "agent",
            "quarantine",
            "--name",
            "ci-agent",
            "--reason",
            "runner drift",
            "--dry-run",
        ],
    );
    assert!(dry_quarantine
        .stdout
        .contains("Dry run: would quarantine agent"));

    env.run_ok(
        &repo,
        [
            "agent",
            "quarantine",
            "--name",
            "ci-agent",
            "--reason",
            "runner drift",
        ],
    );
    let quarantined = env.run_ok(&repo, ["agent", "status", "ci-agent"]);
    assert!(quarantined.stdout.contains("Status: quarantined"));
    assert!(quarantined.stdout.contains("runner drift"));

    let audit = env.run_ok(&repo, ["agent", "audit", "--json"]);
    let audit_json = audit.stdout_json();
    assert_eq!(audit_json["schema_version"], 1);
    assert_eq!(audit_json["action"], "agent.audit");
    assert_eq!(audit_json["quarantined"], 1);
    assert_eq!(audit_json["action_required"], 1);
    assert_eq!(audit_json["warning"], 1);
    let audit_agents = audit_json["agents"]
        .as_array()
        .expect("agent audit inventory");
    let ci_agent = audit_agents
        .iter()
        .find(|agent| agent["agent_id"] == "ci-agent")
        .expect("ci-agent audit entry");
    assert_eq!(ci_agent["record_state"], "registered");
    assert_eq!(ci_agent["status"], "quarantined");
    assert_eq!(ci_agent["local_key_state"], "verified");
    assert_eq!(ci_agent["risk_level"], "warning");
    assert_eq!(ci_agent["action_required"], true);
    assert_eq!(
        ci_agent["recommended_action"],
        "review_quarantine_then_rotate_or_unquarantine"
    );
    assert_eq!(ci_agent["private_fields_present"], false);
    assert!(ci_agent["object_id"].as_str().is_some());
    assert!(ci_agent["public_key_prefix"].as_str().is_some());
    let finding_codes = audit_json["findings"]
        .as_array()
        .expect("agent audit findings")
        .iter()
        .map(|finding| finding["code"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(finding_codes.contains(&"agent_quarantined"));

    let quarantined_audit = env.run_ok(
        &repo,
        [
            "agent",
            "audit",
            "--json",
            "--status",
            "quarantined",
            "--action-required",
        ],
    );
    let quarantined_audit_json = quarantined_audit.stdout_json();
    assert_eq!(quarantined_audit_json["filters"]["status"], "quarantined");
    assert_eq!(quarantined_audit_json["filters"]["action_required"], true);
    assert_eq!(quarantined_audit_json["matching_agents"], 1);
    assert!(quarantined_audit_json["agents"]
        .as_array()
        .expect("filtered audit agents")
        .iter()
        .all(|agent| agent["status"] == "quarantined" && agent["action_required"] == true));
    assert!(quarantined_audit_json["findings"]
        .as_array()
        .expect("filtered audit findings")
        .iter()
        .all(|finding| finding["agent_id"] == "ci-agent"));

    env.write_file(
        &repo.join("agent-fleet.json"),
        r#"{
  "agents": [
    { "action": "register", "name": "fleet-a", "version": "2026-05" },
    { "action": "register", "name": "fleet-b" },
    { "action": "quarantine", "name": "fleet-b", "reason": "runner drift" }
  ]
}
"#,
    );
    let bulk_preview = env.run_ok(
        &repo,
        [
            "agent",
            "--json",
            "bulk",
            "--file",
            "agent-fleet.json",
            "--dry-run",
        ],
    );
    let bulk_preview_json = bulk_preview.stdout_json();
    assert_eq!(bulk_preview_json["schema_version"], 1);
    assert_eq!(bulk_preview_json["action"], "agent.bulk");
    assert_eq!(bulk_preview_json["dry_run"], true);
    assert_eq!(bulk_preview_json["planned_count"], 3);
    assert_eq!(bulk_preview_json["changed_count"], 3);
    assert_eq!(bulk_preview_json["results"][0]["operation"], "register");
    assert_eq!(bulk_preview_json["results"][2]["operation"], "quarantine");
    assert!(bulk_preview_json["results"][0]["object_id"].is_null());

    let missing_after_preview = env.run_ok(&repo, ["agent", "status", "fleet-a", "--json"]);
    assert_eq!(missing_after_preview.stdout_json()["found"], false);

    let bulk_apply = env.run_ok(
        &repo,
        ["agent", "--json", "bulk", "--file", "agent-fleet.json"],
    );
    let bulk_apply_json = bulk_apply.stdout_json();
    assert_eq!(bulk_apply_json["dry_run"], false);
    assert_eq!(bulk_apply_json["changed_count"], 3);
    assert!(bulk_apply_json["results"][0]["object_id"]
        .as_str()
        .is_some());
    let fleet_b = env.run_ok(&repo, ["agent", "status", "fleet-b", "--json"]);
    let fleet_b_json = fleet_b.stdout_json();
    assert_eq!(fleet_b_json["found"], true);
    assert_eq!(fleet_b_json["status"], "quarantined");

    let quarantine_blocked = env.run_fail(
        &repo,
        [
            "ship",
            "--intent",
            intent_id.as_str(),
            "--agent",
            "ci-agent",
            "--evidence",
            "test=pass",
        ],
    );
    assert!(quarantine_blocked
        .combined_output()
        .contains("agent 'ci-agent' is quarantined"));

    env.run_ok(&repo, ["agent", "unquarantine", "--name", "ci-agent"]);
    let unquarantined = env.run_ok(&repo, ["agent", "status", "ci-agent"]);
    assert!(unquarantined.stdout.contains("Status: active"));

    env.run_ok(
        &repo,
        [
            "agent",
            "revoke",
            "--name",
            "ci-agent",
            "--reason",
            "compromised",
        ],
    );
    let revoked = env.run_ok(&repo, ["agent", "status", "ci-agent"]);
    assert!(revoked.stdout.contains("Status: revoked"));
    assert!(revoked.stdout.contains("compromised"));

    let blocked = env.run_fail(
        &repo,
        [
            "ship",
            "--intent",
            intent_id.as_str(),
            "--agent",
            "ci-agent",
            "--evidence",
            "test=pass",
        ],
    );
    assert!(blocked
        .combined_output()
        .contains("agent 'ci-agent' is revoked"));

    env.run_ok(
        &repo,
        [
            "agent",
            "rotate",
            "--name",
            "ci-agent",
            "--version",
            "rotated",
        ],
    );
    let rotated = env.run_ok(&repo, ["agent", "status", "ci-agent"]);
    assert!(rotated.stdout.contains("Status: active"));
    assert!(rotated.stdout.contains("Version: rotated"));

    let shipped = env.run_ok(
        &repo,
        [
            "ship",
            "--intent",
            intent_id.as_str(),
            "--agent",
            "ci-agent",
            "--evidence",
            "test=pass",
        ],
    );
    assert!(shipped.stdout.contains("Capsule: "));
}

#[test]
fn integrate_dry_run_skips_ref_and_worktree_mutation() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("integrate-dry-run");

    env.write_file(&repo.join("app.txt"), "base\n");
    env.run_ok(&repo, ["snapshot", "-m", "Base"]);
    let main_before = ClawStore::open(&repo)
        .expect("open repo")
        .get_ref("heads/main")
        .expect("read main ref")
        .expect("main ref exists");

    env.run_ok(&repo, ["branch", "create", "feature"]);
    env.run_ok(&repo, ["checkout", "feature"]);
    env.write_file(&repo.join("app.txt"), "feature\n");
    env.run_ok(&repo, ["snapshot", "-m", "Feature"]);

    env.run_ok(&repo, ["checkout", "main"]);
    assert_eq!(env.read_file(&repo.join("app.txt")), "base\n");

    let dry_run = env.run_ok(
        &repo,
        [
            "integrate",
            "--right",
            "heads/feature",
            "--dry-run",
            "--json",
        ],
    );
    let dry_run_json = dry_run.stdout_json();
    assert_eq!(dry_run_json["schema_version"], 1);
    assert_eq!(dry_run_json["action"], "integrate");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["clean"], true);
    assert_eq!(dry_run_json["left_ref"], "heads/main");
    assert_eq!(dry_run_json["right_ref"], "heads/feature");
    assert_eq!(dry_run_json["left_revision"], main_before.to_string());
    assert_eq!(dry_run_json["ref_updated"], false);
    assert_eq!(dry_run_json["worktree_updated"], false);
    assert_eq!(dry_run_json["merge_state_written"], false);
    assert_eq!(dry_run_json["conflict_count"], 0);
    assert_eq!(dry_run_json["conflicts"].as_array().map(Vec::len), Some(0));

    let store = ClawStore::open(&repo).expect("open repo after dry-run");
    let main_after = store
        .get_ref("heads/main")
        .expect("read main ref after dry-run")
        .expect("main ref exists after dry-run");
    assert_eq!(main_after, main_before);
    assert_eq!(env.read_file(&repo.join("app.txt")), "base\n");
    assert!(
        !repo.join(".claw").join("MERGE_STATE.toml").exists(),
        "integrate --dry-run must not write merge state"
    );
}

#[test]
fn integrate_conflicts_explain_collided_semantic_regions() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("integrate-conflict-explanations");

    env.write_file(&repo.join("story.txt"), "base\nshared\n");
    env.run_ok(&repo, ["snapshot", "-m", "Base"]);

    env.run_ok(&repo, ["branch", "create", "feature"]);
    env.run_ok(&repo, ["checkout", "feature"]);
    env.write_file(&repo.join("story.txt"), "feature\nshared\n");
    env.run_ok(&repo, ["snapshot", "-m", "Feature edit"]);

    env.run_ok(&repo, ["checkout", "main"]);
    env.write_file(&repo.join("story.txt"), "main\nshared\n");
    env.run_ok(&repo, ["snapshot", "-m", "Main edit"]);

    let dry_run = env.run_ok(
        &repo,
        ["integrate", "--right", "heads/feature", "--dry-run"],
    );
    assert!(dry_run.stdout.contains("CONFLICT: story.txt (text/line)"));
    assert!(dry_run.stdout.contains("reason: left replace at L0"));
    assert!(dry_run.stdout.contains("right replace at L0"));
    assert!(dry_run.stdout.contains("both touched line 1"));
    assert!(dry_run.stdout.contains("collided regions:"));

    let conflicted = env.run_ok(&repo, ["integrate", "--right", "heads/feature"]);
    assert!(conflicted
        .stdout
        .contains("CONFLICT: story.txt (text/line)"));
    assert!(conflicted.stdout.contains("both touched line 1"));

    let status = env.run_ok(&repo, ["status"]);
    assert!(status.stdout.contains("reason: left replace at L0"));
    assert!(status.stdout.contains("both touched line 1"));

    let resolve_list = env.run_ok(&repo, ["resolve", "list"]);
    assert!(resolve_list.stdout.contains("collided regions:"));
    assert!(resolve_list.stdout.contains("left replace at L0"));
    assert!(resolve_list.stdout.contains("right replace at L0"));

    let resolve_json = env.run_ok(&repo, ["resolve", "--json", "list"]);
    let resolve_json = resolve_json.stdout_json();
    assert_eq!(resolve_json["schema_version"], 1);
    assert_eq!(resolve_json["action"], "resolve.list");
    assert_eq!(resolve_json["merge_in_progress"], true);
    assert_eq!(resolve_json["left_ref"], "heads/main");
    assert_eq!(resolve_json["right_ref"], "heads/feature");
    assert_eq!(resolve_json["conflict_count"], 1);
    assert_eq!(resolve_json["ready_count"], 1);
    assert_eq!(resolve_json["unresolved_count"], 0);
    let conflicts = resolve_json["conflicts"]
        .as_array()
        .expect("resolve list conflicts");
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0]["path"], "story.txt");
    assert_eq!(conflicts[0]["codec"], "text/line");
    assert_eq!(conflicts[0]["status"], "ready");
    assert_eq!(conflicts[0]["has_markers"], false);
    assert!(conflicts[0]["reason"]
        .as_str()
        .expect("resolve conflict reason")
        .contains("both touched line 1"));
    assert_eq!(
        conflicts[0]["regions"]
            .as_array()
            .expect("resolve conflict regions")
            .len(),
        2
    );

    let marked = env.run_ok(&repo, ["resolve", "--json", "mark", "story.txt"]);
    let marked = marked.stdout_json();
    assert_eq!(marked["schema_version"], 1);
    assert_eq!(marked["action"], "resolve.mark");
    assert_eq!(marked["path"], "story.txt");
    assert_eq!(marked["marked"], true);
    assert_eq!(marked["remaining_conflict_count"], 0);
    assert_eq!(marked["merge_complete"], true);

    let aborted = env.run_ok(&repo, ["resolve", "--json", "abort"]);
    let aborted = aborted.stdout_json();
    assert_eq!(aborted["schema_version"], 1);
    assert_eq!(aborted["action"], "resolve.abort");
    assert_eq!(aborted["aborted"], true);
    assert_eq!(aborted["restored_ref"], "heads/main");
    assert_eq!(aborted["conflict_count"], 0);
    assert!(
        !repo.join(".claw").join("MERGE_STATE.toml").exists(),
        "resolve abort must remove merge state"
    );
}

#[test]
fn checkout_requires_force_when_worktree_is_dirty() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("checkout-safety");

    env.write_file(&repo.join("tracked.txt"), "base line\n");
    env.run_ok(&repo, ["snapshot", "-m", "Base snapshot"]);
    let created = env.run_ok(&repo, ["branch", "--json", "create", "feature"]);
    let created_json = created.stdout_json();
    assert_eq!(created_json["schema_version"], 1);
    assert_eq!(created_json["action"], "branch.create");
    assert_eq!(created_json["branch"], "feature");
    assert_eq!(created_json["ref"], "heads/feature");
    assert_eq!(created_json["dry_run"], false);
    assert_eq!(created_json["created"], true);
    let feature_target = created_json["target"]
        .as_str()
        .expect("created branch target");
    assert!(feature_target.starts_with("clw_"));

    let dry_created = env.run_ok(
        &repo,
        ["branch", "--json", "create", "preview", "--dry-run"],
    );
    let dry_created_json = dry_created.stdout_json();
    assert_eq!(dry_created_json["schema_version"], 1);
    assert_eq!(dry_created_json["action"], "branch.create");
    assert_eq!(dry_created_json["branch"], "preview");
    assert_eq!(dry_created_json["dry_run"], true);
    assert_eq!(dry_created_json["created"], false);
    assert_eq!(dry_created_json["target"], feature_target);

    let listed = env.run_ok(&repo, ["branch", "--json"]);
    let listed_json = listed.stdout_json();
    assert_eq!(listed_json["schema_version"], 1);
    assert_eq!(listed_json["action"], "branch.list");
    assert_eq!(listed_json["branch_count"], 2);
    assert!(listed_json["branches"]
        .as_array()
        .expect("branch list")
        .iter()
        .any(|branch| branch["name"] == "feature" && branch["target"] == feature_target));

    let delete_preview = env.run_ok(
        &repo,
        ["branch", "--json", "delete", "feature", "--dry-run"],
    );
    let delete_preview_json = delete_preview.stdout_json();
    assert_eq!(delete_preview_json["schema_version"], 1);
    assert_eq!(delete_preview_json["action"], "branch.delete");
    assert_eq!(delete_preview_json["branch"], "feature");
    assert_eq!(delete_preview_json["target"], feature_target);
    assert_eq!(delete_preview_json["dry_run"], true);
    assert_eq!(delete_preview_json["deleted"], false);

    let checkout_preview = env.run_ok(&repo, ["checkout", "--json", "--dry-run", "feature"]);
    let checkout_preview_json = checkout_preview.stdout_json();
    assert_eq!(checkout_preview_json["schema_version"], 1);
    assert_eq!(checkout_preview_json["action"], "checkout");
    assert_eq!(checkout_preview_json["target"], "feature");
    assert_eq!(checkout_preview_json["target_id"], feature_target);
    assert_eq!(checkout_preview_json["dry_run"], true);
    assert_eq!(checkout_preview_json["checked_out"], false);
    assert_eq!(checkout_preview_json["detached"], false);
    assert_eq!(checkout_preview_json["files_written"], 0);
    assert_eq!(checkout_preview_json["updated"], false);

    env.write_file(&repo.join("tracked.txt"), "dirty line\n");
    let blocked = env.run_fail(&repo, ["checkout", "feature"]);
    assert!(blocked.combined_output().contains("uncommitted changes"));

    let forced = env.run_ok(&repo, ["checkout", "--json", "--force", "feature"]);
    let forced_json = forced.stdout_json();
    assert_eq!(forced_json["schema_version"], 1);
    assert_eq!(forced_json["action"], "checkout");
    assert_eq!(forced_json["target"], "feature");
    assert_eq!(forced_json["target_id"], feature_target);
    assert_eq!(forced_json["dry_run"], false);
    assert_eq!(forced_json["checked_out"], true);
    assert_eq!(forced_json["detached"], false);
    assert_eq!(forced_json["files_written"], 1);
    assert_eq!(forced_json["updated"], true);
    assert_eq!(env.read_file(&repo.join("tracked.txt")), "base line\n");
}

#[test]
fn mcp_server_lists_and_calls_claw_tools_over_stdio() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("mcp-stdio");
    let binary = support::claw_binary();

    let mut server = Command::new(binary)
        .current_dir(&repo)
        .arg("mcp")
        .arg("serve")
        .arg("--claw-binary")
        .arg(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn mcp server");
    let mut stdin = server.stdin.take().expect("mcp stdin");
    let stdout = server.stdout.take().expect("mcp stdout");
    let mut reader = BufReader::new(stdout);

    write_json_line(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    );
    let initialize = read_json_line(&mut reader);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "claw");

    write_json_line(
        &mut stdin,
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let tools = read_json_line(&mut reader);
    let names = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"claw_status"));
    assert!(names.contains(&"claw_intent_list"));
    assert!(!names.contains(&"claw_intent_create"));

    write_json_line(
        &mut stdin,
        serde_json::json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{"name":"claw_status","arguments":{}}
        }),
    );
    let status = read_json_line(&mut reader);
    assert_eq!(
        status["result"]["structuredContent"]["branch"].as_str(),
        Some("main")
    );
    assert!(status["result"]["content"][0]["text"]
        .as_str()
        .expect("content text")
        .contains("\"branch\""));

    write_json_line(
        &mut stdin,
        serde_json::json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"tools/call",
            "params":{"name":"claw_intent_create","arguments":{"title":"MCP intent","goal":"prove write guard"}}
        }),
    );
    let blocked = read_json_line(&mut reader);
    assert_eq!(blocked["error"]["code"].as_i64(), Some(-32000));
    assert!(blocked["error"]["message"]
        .as_str()
        .expect("error message")
        .contains("--allow-write"));

    drop(stdin);
    let _ = server.wait();
}

fn write_json_line(stdin: &mut std::process::ChildStdin, value: serde_json::Value) {
    writeln!(stdin, "{value}").expect("write mcp request");
    stdin.flush().expect("flush mcp request");
}

fn read_json_line(reader: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read mcp response");
    serde_json::from_str(&line).expect("mcp response json")
}
