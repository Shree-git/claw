#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

static CLAW_BINARY: OnceLock<PathBuf> = OnceLock::new();
const DAEMON_SPAWN_ATTEMPTS: usize = 5;

pub struct CliTestEnv {
    root: TempDir,
    home_dir: PathBuf,
}

pub struct CommandResult {
    pub status_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct HttpResponse {
    pub status_code: u16,
    pub status_line: String,
    pub body: String,
}

pub struct RunningDaemon {
    child: Child,
    stdout_log: PathBuf,
    stderr_log: PathBuf,
    pub grpc_endpoint: String,
    pub health_addr: String,
}

impl CliTestEnv {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("create integration temp root");
        let home_dir = root.path().join("home");
        fs::create_dir_all(home_dir.join(".claw")).expect("create isolated claw home");
        Self { root, home_dir }
    }

    pub fn temp_root(&self) -> &Path {
        self.root.path()
    }

    pub fn repo_path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    pub fn init_repo(&self, name: &str) -> PathBuf {
        let repo = self.repo_path(name);
        fs::create_dir_all(&repo).expect("create repo dir");
        self.run_ok(
            self.temp_root(),
            ["init", repo.to_str().expect("repo path utf-8")],
        );
        repo
    }

    pub fn write_file(&self, path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directory");
        }
        fs::write(path, content).expect("write test file");
    }

    pub fn read_file(&self, path: &Path) -> String {
        fs::read_to_string(path).expect("read test file")
    }

    pub fn run_ok<I, S>(&self, cwd: &Path, args: I) -> CommandResult
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let rendered = render_args(args);
        let result = self.run(cwd, rendered.iter().map(|arg| arg.as_str()));
        assert!(
            result.status_code == 0,
            "command failed in {}\n$ claw {}\nexit: {}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            rendered.join(" "),
            result.status_code,
            result.stdout,
            result.stderr
        );
        result
    }

    pub fn run_fail<I, S>(&self, cwd: &Path, args: I) -> CommandResult
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let rendered = render_args(args);
        let result = self.run(cwd, rendered.iter().map(|arg| arg.as_str()));
        assert!(
            result.status_code != 0,
            "command unexpectedly succeeded in {}\n$ claw {}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            rendered.join(" "),
            result.stdout,
            result.stderr
        );
        result
    }

    pub fn spawn_daemon<I, S>(&self, repo: &Path, extra_args: I) -> RunningDaemon
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let extra_args = render_args(extra_args);
        let mut last_error = String::new();

        for attempt in 1..=DAEMON_SPAWN_ATTEMPTS {
            let (grpc_addr, health_addr) = reserve_distinct_local_addrs();
            let stdout_log = self
                .temp_root()
                .join(format!("daemon-{}.stdout.log", random_suffix()));
            let stderr_log = self
                .temp_root()
                .join(format!("daemon-{}.stderr.log", random_suffix()));
            let stdout = File::create(&stdout_log).expect("create daemon stdout log");
            let stderr = File::create(&stderr_log).expect("create daemon stderr log");

            let mut command = Command::new(claw_binary());
            command
                .current_dir(repo)
                .arg("daemon")
                .arg("--listen")
                .arg(&grpc_addr)
                .arg("--health-listen")
                .arg(&health_addr)
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));

            apply_isolated_env(&mut command, &self.home_dir);
            for arg in &extra_args {
                command.arg(arg);
            }

            let child = command.spawn().expect("spawn claw daemon");
            let mut daemon = RunningDaemon {
                child,
                stdout_log,
                stderr_log,
                grpc_endpoint: format!("http://{grpc_addr}"),
                health_addr,
            };

            match daemon.wait_until_ready() {
                Ok(()) => return daemon,
                Err(err)
                    if output_indicates_addr_in_use(&err) && attempt < DAEMON_SPAWN_ATTEMPTS =>
                {
                    last_error = err;
                    thread::sleep(Duration::from_millis(50 * attempt as u64));
                }
                Err(err) => panic!("{err}"),
            }
        }

        panic!("daemon did not become ready after address retries: {last_error}");
    }

    fn run<I, S>(&self, cwd: &Path, args: I) -> CommandResult
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = {
            let mut command = Command::new(claw_binary());
            command.current_dir(cwd);
            apply_isolated_env(&mut command, &self.home_dir);
            for arg in args {
                command.arg(arg);
            }
            command.output().expect("run claw command")
        };

        CommandResult {
            status_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl CommandResult {
    pub fn combined_output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    pub fn stdout_json(&self) -> Value {
        serde_json::from_str(&self.stdout).expect("stdout to be valid json")
    }

    pub fn stderr_json(&self) -> Value {
        serde_json::from_str(&self.stderr).expect("stderr to be valid json")
    }

    pub fn value_after(&self, prefix: &str) -> String {
        value_after(&self.combined_output(), prefix)
    }
}

impl RunningDaemon {
    pub fn get(&self, path: &str) -> HttpResponse {
        http_get(&self.health_addr, path)
    }

    pub fn stdout_log(&self) -> String {
        fs::read_to_string(&self.stdout_log).unwrap_or_default()
    }

    pub fn stderr_log(&self) -> String {
        fs::read_to_string(&self.stderr_log).unwrap_or_default()
    }

    fn wait_until_ready(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child_exit_status() {
                return Err(format!(
                    "daemon exited early: {status}\nstdout:\n{}\nstderr:\n{}",
                    self.stdout_log(),
                    self.stderr_log()
                ));
            }

            if let Ok(response) = try_http_get(&self.health_addr, "/v1/health/ready") {
                if response.status_code == 200 {
                    return Ok(());
                }
            }

            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(format!(
                    "daemon did not become ready in time\nstdout:\n{}\nstderr:\n{}",
                    self.stdout_log(),
                    self.stderr_log()
                ));
            }

            thread::sleep(Duration::from_millis(50));
        }
    }

    fn child_exit_status(&mut self) -> Option<i32> {
        self.child
            .try_wait()
            .expect("poll daemon child status")
            .and_then(|status| status.code())
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

pub fn value_after(haystack: &str, prefix: &str) -> String {
    haystack
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix).map(str::to_string))
        .unwrap_or_else(|| panic!("missing prefix '{prefix}' in output:\n{haystack}"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("integration crate under workspace root")
        .to_path_buf()
}

fn claw_binary() -> &'static Path {
    CLAW_BINARY.get_or_init(|| {
        let workspace = workspace_root();
        let status = Command::new("cargo")
            .current_dir(&workspace)
            .args(["build", "-q", "-p", "claw-vcs", "--bin", "claw"])
            .status()
            .expect("build claw binary for integration tests");
        assert!(
            status.success(),
            "cargo build -p claw-vcs failed with {status}"
        );

        let target_dir = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join("target"));
        let binary_name = if cfg!(windows) { "claw.exe" } else { "claw" };
        let path = target_dir.join("debug").join(binary_name);
        assert!(
            path.exists(),
            "built claw binary not found at {}",
            path.display()
        );
        path
    })
}

fn apply_isolated_env(command: &mut Command, home_dir: &Path) {
    command.env("HOME", home_dir);
    command.env("USERPROFILE", home_dir);
    command.env("XDG_CONFIG_HOME", home_dir.join(".config"));
    command.env("NO_COLOR", "1");
    command.env("RUST_BACKTRACE", "1");
}

fn reserve_distinct_local_addrs() -> (String, String) {
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
    (grpc_addr.to_string(), health_addr.to_string())
}

fn output_indicates_addr_in_use(output: &str) -> bool {
    output.contains("Address already in use")
        || output.contains("Only one usage of each socket address")
}

fn random_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("read unix time")
        .as_nanos()
}

fn render_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect()
}

fn http_get(addr: &str, path: &str) -> HttpResponse {
    try_http_get(addr, path)
        .unwrap_or_else(|err| panic!("request http endpoint {}{}: {err}", addr, path))
}

fn try_http_get(addr: &str, path: &str) -> io::Result<HttpResponse> {
    let mut stream = TcpStream::connect(addr)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("set write timeout");

    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;

    let (head, body) = response.split_once("\r\n\r\n").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid http response from {addr}{path}:\n{response}"),
        )
    })?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing http status line for {addr}{path}"),
            )
        })?
        .to_string();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid http status line '{status_line}'"),
            )
        })?;

    Ok(HttpResponse {
        status_code,
        status_line,
        body: body.to_string(),
    })
}
