#![allow(dead_code)]

use claw_core::id::ObjectId;
use claw_sync::proto::sync::sync_service_client::SyncServiceClient;
use claw_sync::proto::sync::HelloRequest;
use serde_json::Value;
use std::fs;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DAEMON_SPAWN_ATTEMPTS: usize = 5;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
}

fn claw_binary_path() -> PathBuf {
    let binary_name = if cfg!(windows) { "claw.exe" } else { "claw" };
    target_dir().join("debug").join(binary_name)
}

fn ensure_claw_binary() -> &'static PathBuf {
    static CLAW_BINARY: OnceLock<PathBuf> = OnceLock::new();
    CLAW_BINARY.get_or_init(|| {
        let path = claw_binary_path();
        if path.exists() {
            return path;
        }

        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(workspace_root())
            .args(["build", "-p", "claw-vcs", "--bin", "claw"])
            .status()
            .expect("build claw binary for live integration tests");
        assert!(
            status.success(),
            "cargo build -p claw-vcs --bin claw failed"
        );
        assert!(
            path.exists(),
            "expected built claw binary at {}",
            path.display()
        );
        path
    })
}

fn free_distinct_local_addrs() -> (SocketAddr, SocketAddr) {
    let grpc_listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral gRPC port");
    let health_listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral health port");
    let grpc_addr = grpc_listener
        .local_addr()
        .expect("read ephemeral gRPC address");
    let health_addr = health_listener
        .local_addr()
        .expect("read ephemeral health address");
    assert_ne!(
        grpc_addr, health_addr,
        "integration daemon gRPC and health listeners must use distinct ports"
    );
    drop((grpc_listener, health_listener));
    (grpc_addr, health_addr)
}

fn child_output_string(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("stdout:\n{stdout}\n\nstderr:\n{stderr}")
}

fn output_indicates_addr_in_use(output: &str) -> bool {
    output.contains("Address already in use")
        || output.contains("Only one usage of each socket address")
}

fn local_test_daemon_config() -> &'static str {
    r#"
config_version = 1

[auth]
require_auth_for_daemon = false
default_profile = "default"

[tls]
require_for_non_localhost = true

[timeouts]
io_ms = 10000
git_bridge_ms = 15000
policy_eval_ms = 5000

[retries]
idempotent_only = true
max_attempts = 4
base_backoff_ms = 100
max_backoff_ms = 2000
jitter = true

[queues]
worker_pool_size = 8
queue_capacity = 1024
backpressure = true

[telemetry]
structured_logs = true
correlation_ids = true
metrics = true
traces = true

[policy]
fail_closed_integrate = true
fail_closed_ship = true

[backup]
snapshot_interval_min = 60
verify_integrity_on_startup = false
strict_startup_checks = false
"#
}

fn configure_isolated_home(command: &mut Command, root: &Path) {
    let home = root.join(".claw-test-home");
    fs::create_dir_all(home.join(".config")).expect("create isolated test config dir");
    fs::create_dir_all(home.join(".local").join("share")).expect("create isolated test data dir");
    command
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local").join("share"));
}

pub fn read_workspace_file(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

pub fn read_workspace_json(relative: &str) -> Value {
    let path = workspace_root().join(relative);
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("parse {} as json: {err}", path.display()))
}

pub fn init_temp_repo() -> TempDir {
    let temp = tempfile::tempdir().expect("create temp repo");
    claw_store::ClawStore::init(temp.path()).expect("initialize claw repo");
    write_repo_config(temp.path(), local_test_daemon_config());
    temp
}

pub fn write_repo_config(root: &Path, content: &str) {
    let path = root.join(".claw").join("config.toml");
    fs::write(&path, content).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
}

pub fn proto_object_id(id: &ObjectId) -> claw_sync::proto::common::ObjectId {
    claw_sync::proto::common::ObjectId {
        hash: id.as_bytes().to_vec(),
    }
}

pub fn run_claw_command(cwd: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(ensure_claw_binary());
    configure_isolated_home(&mut command, cwd);
    command
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("run claw {:?}: {err}", args))
}

pub struct RawHttpResponse {
    pub status_line: String,
    pub headers: String,
    pub body: Vec<u8>,
}

pub async fn raw_http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<RawHttpResponse> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n",
        host = addr
    );
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    if !body.is_empty() {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");

    stream.write_all(request.as_bytes()).await?;
    if !body.is_empty() {
        stream.write_all(body).await?;
    }
    stream.shutdown().await?;

    let mut response_bytes = Vec::new();
    stream.read_to_end(&mut response_bytes).await?;
    let response = String::from_utf8_lossy(&response_bytes);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing http separator"))?;

    Ok(RawHttpResponse {
        status_line: head.lines().next().unwrap_or_default().to_string(),
        headers: head.to_string(),
        body: body.as_bytes().to_vec(),
    })
}

pub fn json_body(response: &RawHttpResponse) -> Value {
    serde_json::from_slice(&response.body).expect("response body should be valid json")
}

pub struct LiveDaemon {
    child: Option<Child>,
    pub grpc_addr: SocketAddr,
    pub health_addr: SocketAddr,
    pub grpc_endpoint: String,
}

impl LiveDaemon {
    pub async fn spawn(root: &Path, extra_daemon_args: &[&str]) -> Self {
        let mut last_error = String::new();
        for attempt in 1..=DAEMON_SPAWN_ATTEMPTS {
            let (grpc_addr, health_addr) = free_distinct_local_addrs();

            let mut command = Command::new(ensure_claw_binary());
            configure_isolated_home(&mut command, root);
            let child = command
                .current_dir(root)
                .args(["--profile", "dev", "daemon"])
                .arg("--listen")
                .arg(grpc_addr.to_string())
                .arg("--health-listen")
                .arg(health_addr.to_string())
                .args(extra_daemon_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn live claw daemon");

            let grpc_endpoint = format!("http://{grpc_addr}");
            let mut daemon = Self {
                child: Some(child),
                grpc_addr,
                health_addr,
                grpc_endpoint,
            };
            match daemon.wait_until_ready().await {
                Ok(()) => return daemon,
                Err(err)
                    if output_indicates_addr_in_use(&err) && attempt < DAEMON_SPAWN_ATTEMPTS =>
                {
                    last_error = err;
                    tokio::time::sleep(Duration::from_millis(50 * attempt as u64)).await;
                }
                Err(err) => panic!("{err}"),
            }
        }

        panic!("daemon did not become ready after address retries: {last_error}");
    }

    async fn wait_until_ready(&mut self) -> Result<(), String> {
        let mut last_health_error = String::new();
        let mut last_grpc_error = String::new();

        for _ in 0..200 {
            if let Some(status) = self
                .child
                .as_mut()
                .expect("daemon child should exist")
                .try_wait()
                .expect("poll daemon child")
            {
                let output = self
                    .child
                    .take()
                    .expect("take exited daemon child")
                    .wait_with_output()
                    .expect("collect daemon output");
                return Err(format!(
                    "daemon exited before becoming ready with status {status}: {}",
                    child_output_string(&output)
                ));
            }

            match raw_http_request(self.health_addr, "GET", "/v1/health/live", &[], &[]).await {
                Ok(response) if response.status_line == "HTTP/1.1 200 OK" => {
                    match SyncServiceClient::connect(self.grpc_endpoint.clone()).await {
                        Ok(mut client) => {
                            let hello = client
                                .hello(tonic::Request::new(HelloRequest {
                                    client_version: env!("CARGO_PKG_VERSION").to_string(),
                                    capabilities: vec!["partial-clone".to_string()],
                                }))
                                .await;
                            if hello.is_ok() {
                                return Ok(());
                            }
                            last_grpc_error = hello.err().unwrap().to_string();
                        }
                        Err(err) => last_grpc_error = err.to_string(),
                    }
                }
                Ok(response) => {
                    last_health_error =
                        format!("unexpected health status {}", response.status_line);
                }
                Err(err) => last_health_error = err.to_string(),
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let mut child = self.child.take().expect("take daemon child after timeout");
        let _ = child.kill();
        let output = child
            .wait_with_output()
            .expect("collect daemon output after timeout");
        Err(format!(
            "daemon did not become ready; last health error: {last_health_error}; last gRPC error: {last_grpc_error}; {}",
            child_output_string(&output)
        ))
    }
}

impl Drop for LiveDaemon {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
