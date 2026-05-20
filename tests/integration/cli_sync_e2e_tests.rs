mod support;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::CliTestEnv;

use serde_json::Value;

#[test]
fn daemon_auth_health_and_sync_round_trip_work_via_built_binary() {
    let env = CliTestEnv::new();
    let source = env.init_repo("source");
    let audit_log = env.temp_root().join("daemon.audit.jsonl");

    env.run_ok(
        env.temp_root(),
        [
            "auth",
            "token",
            "set",
            "super-secret-token",
            "--profile",
            "e2e",
            "--base-url",
            "http://127.0.0.1",
        ],
    );

    env.write_file(&source.join("sync.txt"), "version one\n");
    env.run_ok(&source, ["snapshot", "-m", "Seed remote repository"]);

    let daemon = env.spawn_daemon(
        &source,
        vec![
            OsString::from("--auth-profile"),
            OsString::from("e2e"),
            OsString::from("--audit-log"),
            audit_log.as_os_str().to_os_string(),
        ],
    );

    let live = daemon.get("/v1/health/live");
    assert_eq!(live.status_code, 200);
    assert!(live.body.contains("\"status\":\"live\""));

    let ready = daemon.get("/v1/health/ready");
    assert_eq!(ready.status_code, 200);
    assert!(ready.body.contains("\"status\":\"ready\""));

    let metrics = daemon.get("/v1/metrics");
    assert_eq!(metrics.status_code, 200);
    assert!(metrics.status_line.contains("200"));
    assert!(metrics
        .body
        .contains("claw_daemon_http_request_latency_seconds"));
    assert!(metrics.body.contains("claw_daemon_auth_failures_total"));

    let unauth_clone = env.repo_path("clone-without-token");
    let denied = env.run_fail(
        env.temp_root(),
        [
            "sync",
            "clone",
            daemon.grpc_endpoint.as_str(),
            unauth_clone.to_str().expect("clone path utf-8"),
        ],
    );
    assert!(
        denied.combined_output().contains("missing bearer token")
            || denied.combined_output().contains("Unauthenticated")
    );

    env.run_ok(
        env.temp_root(),
        [
            "auth",
            "token",
            "set",
            "stale-or-wrong-token",
            "--profile",
            "stale",
            "--base-url",
            "http://127.0.0.1",
        ],
    );
    let stale_token_clone = env.repo_path("clone-with-stale-token");
    let stale_denied = env.run_fail(
        env.temp_root(),
        [
            "sync",
            "clone",
            "--token-profile",
            "stale",
            daemon.grpc_endpoint.as_str(),
            stale_token_clone.to_str().expect("clone path utf-8"),
        ],
    );
    assert!(
        stale_denied
            .combined_output()
            .contains("invalid bearer token")
            || stale_denied.combined_output().contains("Unauthenticated")
    );

    let auth_metrics = daemon.get("/v1/metrics");
    assert_eq!(auth_metrics.status_code, 200);
    assert!(auth_metrics.body.contains("reason=\"missing\""));
    assert!(auth_metrics.body.contains("reason=\"invalid\""));

    let clone_a = env.repo_path("clone-a");
    let clone_b = env.repo_path("clone-b");

    let clone_a_out = env.run_ok(
        env.temp_root(),
        [
            "sync",
            "clone",
            "--token-profile",
            "e2e",
            daemon.grpc_endpoint.as_str(),
            clone_a.to_str().expect("clone-a path utf-8"),
        ],
    );
    assert!(clone_a_out.stdout.contains("Cloned"));
    assert_eq!(env.read_file(&clone_a.join("sync.txt")), "version one\n");

    env.run_ok(
        env.temp_root(),
        [
            "sync",
            "clone",
            "--token-profile",
            "e2e",
            daemon.grpc_endpoint.as_str(),
            clone_b.to_str().expect("clone-b path utf-8"),
        ],
    );
    assert_eq!(env.read_file(&clone_b.join("sync.txt")), "version one\n");

    let remotes = env.run_ok(&clone_a, ["remote", "list"]);
    assert!(remotes.stdout.contains("origin\tgrpc\t"));
    assert!(remotes.stdout.contains("\te2e"));

    env.write_file(&clone_a.join("sync.txt"), "version two from clone a\n");
    env.run_ok(&clone_a, ["snapshot", "-m", "Advance remote head"]);

    let dry_push = env.run_ok(
        &clone_a,
        ["sync", "push", "--remote", "origin", "--dry-run"],
    );
    assert!(dry_push.stdout.contains("Dry run: would push"));
    assert!(dry_push.stdout.contains("Remote ref update skipped."));

    let dry_pull = env.run_ok(&clone_b, ["sync", "pull", "--remote", "origin"]);
    assert!(dry_pull.stdout.contains("Updated heads/main"));
    assert_eq!(env.read_file(&clone_b.join("sync.txt")), "version one\n");

    let pushed = env.run_ok(&clone_a, ["sync", "push", "--remote", "origin"]);
    assert!(pushed.stdout.contains("Pushed heads/main to origin"));

    assert_eq!(env.read_file(&clone_b.join("sync.txt")), "version one\n");
    let pulled = env.run_ok(&clone_b, ["sync", "pull", "--remote", "origin"]);
    assert!(pulled.stdout.contains("Working tree updated."));
    assert_eq!(
        env.read_file(&clone_b.join("sync.txt")),
        "version two from clone a\n"
    );

    let audit_events = read_audit_events(&audit_log);
    assert!(
        audit_events.iter().any(|event| event["action"] == "hello"
            && event["outcome"] == "denied"
            && event["reason"] == "missing bearer token"),
        "expected missing-token denied audit event, got {audit_events:#?}"
    );
    assert!(
        audit_events.iter().any(|event| event["action"] == "hello"
            && event["outcome"] == "denied"
            && event["reason"] == "invalid bearer token"),
        "expected invalid-token denied audit event, got {audit_events:#?}"
    );
    assert!(
        audit_events.iter().any(|event| event["action"] == "hello"
            && event["outcome"] == "allowed"
            && event["subject"]["principal"] == "daemon-token"),
        "expected allowed hello audit event, got {audit_events:#?}"
    );
    assert!(
        audit_events
            .iter()
            .any(|event| event["action"] == "update_refs"
                && event["outcome"] == "allowed"
                && event["resource"]
                    .as_str()
                    .is_some_and(|resource| resource.contains("heads/main"))
                && event["subject"]["principal"] == "daemon-token"),
        "expected allowed update_refs audit event, got {audit_events:#?}"
    );

    let audit_log_contents = fs::read_to_string(&audit_log).expect("read audit log");
    let sensitive_outputs = [
        denied.combined_output(),
        stale_denied.combined_output(),
        clone_a_out.combined_output(),
        auth_metrics.body,
        daemon.stdout_log(),
        daemon.stderr_log(),
        audit_log_contents,
    ]
    .join("\n");
    for secret in ["super-secret-token", "stale-or-wrong-token"] {
        assert!(
            !sensitive_outputs.contains(secret),
            "secret token leaked into daemon or CLI output: {secret}"
        );
    }
}

#[test]
fn daemon_mtls_clone_requires_client_certificate() {
    let Some(tls) = generate_tls_fixture() else {
        eprintln!("skipping mTLS integration test because openssl is unavailable");
        return;
    };

    let env = CliTestEnv::new();
    let source = env.init_repo("mtls-source");

    env.run_ok(
        env.temp_root(),
        [
            "auth",
            "token",
            "set",
            "mtls-secret-token",
            "--profile",
            "mtls",
            "--base-url",
            "https://127.0.0.1",
        ],
    );

    env.write_file(&source.join("secure-sync.txt"), "mtls version one\n");
    env.run_ok(&source, ["snapshot", "-m", "Seed mTLS remote repository"]);

    let daemon = env.spawn_daemon(&source, tls.daemon_args("mtls"));
    let tls_endpoint = daemon.grpc_endpoint.replacen("http://", "https://", 1);

    let denied_clone = env.repo_path("mtls-clone-without-client-cert");
    let denied = env.run_fail(
        env.temp_root(),
        tls.sync_clone_args_without_client_cert("mtls", &tls_endpoint, &denied_clone),
    );
    let denied_output = denied.combined_output();
    assert!(
        denied_output.contains("transport error")
            || denied_output.contains("ConnectionFailed")
            || denied_output.contains("certificate")
            || denied_output.contains("tls")
            || denied_output.contains("operation was canceled")
            || denied_output.contains("connection closed"),
        "expected TLS/client certificate failure, got:\n{denied_output}"
    );

    let clone_path = env.repo_path("mtls-clone");
    let cloned = env.run_ok(
        env.temp_root(),
        tls.sync_clone_args("mtls", &tls_endpoint, &clone_path),
    );
    assert!(cloned.stdout.contains("Cloned"));
    assert_eq!(
        env.read_file(&clone_path.join("secure-sync.txt")),
        "mtls version one\n"
    );
}

struct TlsFixture {
    _dir: tempfile::TempDir,
    ca_cert: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
    client_cert: PathBuf,
    client_key: PathBuf,
}

impl TlsFixture {
    fn daemon_args(&self, auth_profile: &str) -> Vec<OsString> {
        vec![
            "--auth-profile".into(),
            auth_profile.into(),
            "--tls-cert".into(),
            self.server_cert.as_os_str().to_os_string(),
            "--tls-key".into(),
            self.server_key.as_os_str().to_os_string(),
            "--client-ca-cert".into(),
            self.ca_cert.as_os_str().to_os_string(),
        ]
    }

    fn sync_clone_args(
        &self,
        token_profile: &str,
        endpoint: &str,
        clone_path: &Path,
    ) -> Vec<OsString> {
        let mut args =
            self.sync_clone_args_without_client_cert(token_profile, endpoint, clone_path);
        args.splice(
            5..5,
            [
                OsString::from("--client-cert"),
                self.client_cert.as_os_str().to_os_string(),
                OsString::from("--client-key"),
                self.client_key.as_os_str().to_os_string(),
            ],
        );
        args
    }

    fn sync_clone_args_without_client_cert(
        &self,
        token_profile: &str,
        endpoint: &str,
        clone_path: &Path,
    ) -> Vec<OsString> {
        vec![
            "sync".into(),
            "--tls-ca-cert".into(),
            self.ca_cert.as_os_str().to_os_string(),
            "--tls-domain".into(),
            "localhost".into(),
            "clone".into(),
            "--token-profile".into(),
            token_profile.into(),
            endpoint.into(),
            clone_path.as_os_str().to_os_string(),
        ]
    }
}

fn generate_tls_fixture() -> Option<TlsFixture> {
    if Command::new("openssl").arg("version").output().is_err() {
        return None;
    }

    let dir = tempfile::tempdir().expect("create TLS fixture temp dir");
    let ca_key = dir.path().join("ca.key");
    let ca_cert = dir.path().join("ca.crt");
    let ca_config = dir.path().join("ca.cnf");
    let server_key = dir.path().join("server.key");
    let server_csr = dir.path().join("server.csr");
    let server_cert = dir.path().join("server.crt");
    let server_config = dir.path().join("server.cnf");
    let client_key = dir.path().join("client.key");
    let client_csr = dir.path().join("client.csr");
    let client_cert = dir.path().join("client.crt");
    let client_config = dir.path().join("client.cnf");

    fs::write(
        &ca_config,
        r#"[req]
distinguished_name = dn
prompt = no
x509_extensions = v3_ca

[dn]
CN = Claw Test CA

[v3_ca]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always,issuer
"#,
    )
    .expect("write CA openssl config");

    fs::write(
        &server_config,
        r#"[req]
distinguished_name = dn
prompt = no
req_extensions = v3_req

[dn]
CN = localhost

[v3_req]
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
IP.1 = 127.0.0.1
"#,
    )
    .expect("write server openssl config");

    fs::write(
        &client_config,
        r#"[req]
distinguished_name = dn
prompt = no
req_extensions = v3_req

[dn]
CN = claw-test-client

[v3_req]
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = clientAuth
"#,
    )
    .expect("write client openssl config");

    run_openssl(
        dir.path(),
        [
            "req".into(),
            "-x509".into(),
            "-newkey".into(),
            "rsa:2048".into(),
            "-nodes".into(),
            "-sha256".into(),
            "-days".into(),
            "2".into(),
            "-config".into(),
            ca_config.as_os_str().to_os_string(),
            "-keyout".into(),
            ca_key.as_os_str().to_os_string(),
            "-out".into(),
            ca_cert.as_os_str().to_os_string(),
        ],
    );
    run_openssl(
        dir.path(),
        [
            "req".into(),
            "-newkey".into(),
            "rsa:2048".into(),
            "-nodes".into(),
            "-keyout".into(),
            server_key.as_os_str().to_os_string(),
            "-out".into(),
            server_csr.as_os_str().to_os_string(),
            "-config".into(),
            server_config.as_os_str().to_os_string(),
        ],
    );
    run_openssl(
        dir.path(),
        [
            "x509".into(),
            "-req".into(),
            "-in".into(),
            server_csr.as_os_str().to_os_string(),
            "-CA".into(),
            ca_cert.as_os_str().to_os_string(),
            "-CAkey".into(),
            ca_key.as_os_str().to_os_string(),
            "-CAcreateserial".into(),
            "-out".into(),
            server_cert.as_os_str().to_os_string(),
            "-days".into(),
            "2".into(),
            "-sha256".into(),
            "-extensions".into(),
            "v3_req".into(),
            "-extfile".into(),
            server_config.as_os_str().to_os_string(),
        ],
    );
    run_openssl(
        dir.path(),
        [
            "req".into(),
            "-newkey".into(),
            "rsa:2048".into(),
            "-nodes".into(),
            "-keyout".into(),
            client_key.as_os_str().to_os_string(),
            "-out".into(),
            client_csr.as_os_str().to_os_string(),
            "-config".into(),
            client_config.as_os_str().to_os_string(),
        ],
    );
    run_openssl(
        dir.path(),
        [
            "x509".into(),
            "-req".into(),
            "-in".into(),
            client_csr.as_os_str().to_os_string(),
            "-CA".into(),
            ca_cert.as_os_str().to_os_string(),
            "-CAkey".into(),
            ca_key.as_os_str().to_os_string(),
            "-CAcreateserial".into(),
            "-out".into(),
            client_cert.as_os_str().to_os_string(),
            "-days".into(),
            "2".into(),
            "-sha256".into(),
            "-extensions".into(),
            "v3_req".into(),
            "-extfile".into(),
            client_config.as_os_str().to_os_string(),
        ],
    );

    Some(TlsFixture {
        _dir: dir,
        ca_cert,
        server_cert,
        server_key,
        client_cert,
        client_key,
    })
}

fn run_openssl<I>(cwd: &Path, args: I)
where
    I: IntoIterator<Item = OsString>,
{
    let rendered: Vec<OsString> = args.into_iter().collect();
    let output = Command::new("openssl")
        .current_dir(cwd)
        .args(&rendered)
        .output()
        .expect("run openssl");
    assert!(
        output.status.success(),
        "openssl {:?} failed\nstdout:\n{}\nstderr:\n{}",
        rendered,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_audit_events(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .expect("read daemon audit log")
        .lines()
        .map(|line| serde_json::from_str(line).expect("audit log line to be valid json"))
        .collect()
}
