mod support;

use support::CliTestEnv;

fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys = value
        .as_object()
        .expect("json value to be an object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn assert_cli_request_id_format(value: &str) {
    let parts: Vec<&str> = value.split('_').collect();
    assert_eq!(
        parts.len(),
        3,
        "request_id must use req_<millis>_<counter> format: {value}"
    );
    assert_eq!(parts[0], "req");
    assert!(
        parts[1].parse::<u128>().is_ok(),
        "request_id timestamp segment must be numeric: {value}"
    );
    assert!(
        parts[2].parse::<u64>().is_ok(),
        "request_id counter segment must be numeric: {value}"
    );
}

#[test]
fn json_error_format_wraps_failures_with_a_machine_readable_envelope() {
    let env = CliTestEnv::new();

    let result = env.run_fail(env.temp_root(), ["--error-format", "json", "status"]);
    let error = result.stderr_json();

    assert_eq!(
        sorted_keys(&error),
        vec![
            "code",
            "details",
            "exit_code",
            "message",
            "reason",
            "remediation",
            "request_id",
            "schema_version"
        ]
    );
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["code"], "NOT_REPOSITORY");
    assert!(error["message"]
        .as_str()
        .expect("error message to be a string")
        .contains("not in a claw repository"));
    assert!(error["reason"]
        .as_str()
        .expect("reason to be a string")
        .contains(".claw"));
    assert_cli_request_id_format(
        error["request_id"]
            .as_str()
            .expect("request_id to be a string"),
    );
    assert_eq!(error["exit_code"], 3);
    assert!(error["remediation"]
        .as_str()
        .expect("remediation to be a string")
        .contains("claw init"));
}

#[test]
fn human_error_format_includes_request_id_for_support_correlation() {
    let env = CliTestEnv::new();

    let result = env.run_fail(env.temp_root(), ["status"]);

    assert_eq!(result.status_code, 3);
    assert!(result.stderr.contains("error[NOT_REPOSITORY]:"));
    let request_id = result
        .stderr
        .lines()
        .find_map(|line| line.strip_prefix("request_id: "))
        .expect("human diagnostic must include request_id line");
    assert_cli_request_id_format(request_id);
    assert!(result.stderr.contains("why:"));
    assert!(result.stderr.contains("try:"));
}

#[test]
fn json_error_format_wraps_usage_failures() {
    let env = CliTestEnv::new();

    let result = env.run_fail(
        env.temp_root(),
        ["--error-format", "json", "--not-a-real-flag"],
    );
    let error = result.stderr_json();

    assert_eq!(error["code"], "USAGE_ERROR");
    assert_eq!(error["schema_version"], 1);
    assert!(error["message"]
        .as_str()
        .expect("error message to be a string")
        .contains("--not-a-real-flag"));
    assert!(error["reason"]
        .as_str()
        .expect("reason to be a string")
        .contains("command line"));
    assert_cli_request_id_format(
        error["request_id"]
            .as_str()
            .expect("request_id to be a string"),
    );
    assert_eq!(error["exit_code"], 2);
    assert_eq!(error["details"]["kind"], "UnknownArgument");
    assert!(error["remediation"]
        .as_str()
        .expect("remediation to be a string")
        .contains("claw --help"));
}

#[test]
fn json_error_format_classifies_remote_failures() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("json-remote-error");

    let result = env.run_fail(
        &repo,
        [
            "--error-format",
            "json",
            "sync",
            "pull",
            "--remote",
            "origin",
        ],
    );
    let error = result.stderr_json();

    assert_eq!(
        sorted_keys(&error),
        vec![
            "code",
            "details",
            "exit_code",
            "message",
            "reason",
            "remediation",
            "request_id",
            "schema_version"
        ]
    );
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["code"], "REMOTE_ERROR");
    assert_eq!(error["exit_code"], 7);
    assert!(error["remediation"]
        .as_str()
        .expect("remediation to be a string")
        .contains("claw remote list"));
}

#[test]
fn malformed_auth_config_used_by_sync_is_a_config_error() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("json-auth-config-error");
    env.run_ok(
        &repo,
        [
            "remote",
            "add",
            "origin",
            "http://127.0.0.1:50051",
            "--kind",
            "grpc",
            "--token-profile",
            "prod",
        ],
    );
    let auth_path = env.temp_root().join("home").join(".claw").join("auth.toml");
    let broken_auth = "[profiles.prod\naccess_token = \"old-token\"\n";
    env.write_file(&auth_path, broken_auth);

    let result = env.run_fail(
        &repo,
        [
            "--error-format",
            "json",
            "sync",
            "pull",
            "--remote",
            "origin",
        ],
    );
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
        "sync token resolution must not rewrite malformed auth.toml"
    );
}

#[test]
fn json_error_format_auth_remediation_prefers_stdin_token_import() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("json-auth-remediation");
    env.run_ok(
        &repo,
        [
            "remote",
            "add",
            "origin",
            "http://127.0.0.1:50051",
            "--kind",
            "grpc",
            "--token-profile",
            "prod",
        ],
    );

    let result = env.run_fail(
        &repo,
        [
            "--error-format",
            "json",
            "sync",
            "pull",
            "--remote",
            "origin",
        ],
    );
    let error = result.stderr_json();

    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["code"], "AUTH_ERROR");
    assert_eq!(error["exit_code"], 6);
    assert!(error["remediation"]
        .as_str()
        .expect("remediation")
        .contains("claw auth token set --stdin"));
    assert!(!error["remediation"]
        .as_str()
        .expect("remediation")
        .contains("claw auth token set <token>"));
}

#[test]
fn json_error_format_classifies_invalid_ref_names() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("json-invalid-ref-error");
    env.write_file(&repo.join("README.md"), "hello\n");
    env.run_ok(&repo, ["snapshot", "-m", "initial"]);

    let result = env.run_fail(&repo, ["--error-format", "json", "branch", "create", "CON"]);
    let error = result.stderr_json();

    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["code"], "INVALID_REF_NAME");
    assert_eq!(error["exit_code"], 1);
    assert_eq!(error["details"]["ref_name"], "heads/CON");
    assert!(error["reason"]
        .as_str()
        .expect("reason")
        .contains("not portable"));
    assert!(error["remediation"]
        .as_str()
        .expect("remediation")
        .contains("heads/main"));
}

#[test]
fn json_error_format_classifies_ref_name_case_collisions() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("json-ref-collision-error");
    env.write_file(&repo.join("README.md"), "hello\n");
    env.run_ok(&repo, ["snapshot", "-m", "initial"]);

    let result = env.run_fail(
        &repo,
        ["--error-format", "json", "branch", "create", "MAIN"],
    );
    let error = result.stderr_json();

    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["code"], "REF_NAME_COLLISION");
    assert_eq!(error["exit_code"], 1);
    assert_eq!(error["details"]["requested"], "heads/MAIN");
    assert_eq!(error["details"]["existing"], "heads/main");
    assert!(error["reason"]
        .as_str()
        .expect("reason")
        .contains("case-insensitive"));
    assert!(error["remediation"]
        .as_str()
        .expect("remediation")
        .contains("letter case"));
}
