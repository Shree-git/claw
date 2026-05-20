use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::Args;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};
use serde::Deserialize;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;
use tonic::metadata::MetadataValue;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic::{Request, Status};

use claw_store::ClawStore;
use claw_sync::capsule_service::CapsuleServer;
use claw_sync::change_service::ChangeServer;
use claw_sync::event_service::{EventBus, EventServer};
use claw_sync::intent_service::IntentServer;
use claw_sync::proto::capsule::capsule_service_server::CapsuleServiceServer;
use claw_sync::proto::change::change_service_server::ChangeServiceServer;
use claw_sync::proto::event::event_stream_service_server::EventStreamServiceServer;
use claw_sync::proto::intent::intent_service_server::IntentServiceServer;
use claw_sync::proto::sync::sync_service_server::SyncServiceServer;
use claw_sync::proto::workstream::workstream_service_server::WorkstreamServiceServer;
use claw_sync::protocol::{server_capabilities, SYNC_PROTOCOL_VERSION};
use claw_sync::security::{
    now_ms, request_id_from_request, subject_from_request, AuditEvent, AuditSink,
    AuthorizationAction, AuthorizationRole, AuthorizationScope, Authorizer, JsonlAuditSink,
    RateLimiter, ReplayProtectionConfig, RoleBasedAuthorizer, TeeAuditSink, TracingAuditSink,
    PRINCIPAL_METADATA_KEY, TOKEN_ID_METADATA_KEY,
};
use claw_sync::server::{SyncServer, SyncServerOptions};
use claw_sync::workstream_service::WorkstreamServer;

use crate::auth_store;
use crate::commands::RuntimeOptions;
use crate::config::{self, find_repo_root};

#[derive(Args)]
pub struct DaemonArgs {
    /// Listen address
    #[arg(short, long, default_value = "[::1]:50051")]
    listen: String,
    /// HTTP health listen address
    #[arg(long, default_value = "[::1]:50052")]
    health_listen: String,
    /// Allow unauthenticated health and metrics endpoints to bind outside localhost in production
    #[arg(long)]
    allow_public_health: bool,
    /// Use stdio instead of TCP (for embedded use)
    #[arg(long)]
    stdio: bool,
    /// Require bearer auth token for all gRPC requests
    #[arg(long)]
    auth_token: Option<String>,
    /// Read bearer auth token from a saved auth profile
    #[arg(long)]
    auth_profile: Option<String>,
    /// Principal name attached to the configured bearer token for daemon authorization
    #[arg(long, default_value = "daemon-token")]
    auth_principal: String,
    /// Daemon authorization role for the configured bearer token
    #[arg(long, default_value = "admin")]
    auth_role: String,
    /// Additional daemon authorization scope for the configured bearer token
    #[arg(long = "auth-scope")]
    auth_scopes: Vec<String>,
    /// Require replay nonce metadata on sync ref/object mutations
    #[arg(long)]
    require_replay_nonce: bool,
    /// Maximum accepted sync requests per minute for this daemon
    #[arg(long)]
    rate_limit_per_minute: Option<u32>,
    /// Maximum byte length for one pushed object chunk
    #[arg(long)]
    max_push_chunk_bytes: Option<usize>,
    /// Maximum aggregate byte length for one push request
    #[arg(long)]
    max_push_request_bytes: Option<usize>,
    /// TLS certificate (PEM) path
    #[arg(long)]
    tls_cert: Option<PathBuf>,
    /// TLS private key (PEM) path
    #[arg(long)]
    tls_key: Option<PathBuf>,
    /// Client CA certificate (PEM) path; enables required client certificate auth for gRPC TLS
    #[arg(long)]
    client_ca_cert: Option<PathBuf>,
    /// Append authorization audit events to this JSON Lines file
    #[arg(long)]
    audit_log: Option<PathBuf>,
}

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
const MAX_HEALTH_REQUEST_BODY_BYTES: usize = 64 * 1024;
const GRPC_MESSAGE_OVERHEAD_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
struct BearerAuthInterceptor {
    expected_header: Arc<str>,
    principal: Arc<str>,
    token_id: Arc<str>,
    metrics: Arc<DaemonMetrics>,
    audit_sink: Arc<dyn AuditSink>,
    auth_failure_limiter: Option<Arc<Mutex<RateLimiter>>>,
}

impl BearerAuthInterceptor {
    fn new(
        token: String,
        principal: String,
        metrics: Arc<DaemonMetrics>,
        audit_sink: Arc<dyn AuditSink>,
    ) -> Self {
        Self {
            token_id: Arc::<str>::from(bearer_token_id(&token)),
            expected_header: Arc::<str>::from(format!("Bearer {token}")),
            principal: Arc::<str>::from(principal),
            metrics,
            audit_sink,
            auth_failure_limiter: None,
        }
    }

    fn with_auth_failure_rate_limit(mut self, limit_per_minute: Option<u32>) -> Self {
        self.auth_failure_limiter = limit_per_minute
            .map(|limit| Arc::new(Mutex::new(RateLimiter::per_minute(limit, Instant::now()))));
        self
    }

    fn record_auth_denied(&self, request: &Request<()>, reason: &'static str) {
        self.audit_sink.record(AuditEvent::denied(
            now_ms(),
            request_id_from_request(request),
            subject_from_request(request),
            AuthorizationAction::Hello,
            None,
            reason,
        ));
    }

    fn enforce_auth_failure_rate_limit(&self, request: &Request<()>) -> Result<(), Status> {
        let Some(limiter) = &self.auth_failure_limiter else {
            return Ok(());
        };
        let mut limiter = limiter
            .lock()
            .map_err(|_| Status::internal("auth failure limiter lock poisoned"))?;
        if limiter.try_acquire(Instant::now()) {
            return Ok(());
        }

        self.metrics
            .auth_failures
            .with_label_values(&["throttled"])
            .inc();
        self.record_auth_denied(request, "auth failure rate limit exceeded");
        Err(Status::resource_exhausted(
            "auth failure rate limit exceeded",
        ))
    }
}

impl tonic::service::Interceptor for BearerAuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let provided = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok());

        match provided {
            Some(value) if value == self.expected_header.as_ref() => {
                request.metadata_mut().remove(PRINCIPAL_METADATA_KEY);
                request.metadata_mut().remove(TOKEN_ID_METADATA_KEY);
                let principal = MetadataValue::try_from(self.principal.as_ref())
                    .map_err(|_| Status::internal("invalid configured auth principal"))?;
                let token_id = MetadataValue::try_from(self.token_id.as_ref())
                    .map_err(|_| Status::internal("invalid configured auth token id"))?;
                request
                    .metadata_mut()
                    .insert(PRINCIPAL_METADATA_KEY, principal);
                request
                    .metadata_mut()
                    .insert(TOKEN_ID_METADATA_KEY, token_id);
                Ok(request)
            }
            Some(_) => {
                self.enforce_auth_failure_rate_limit(&request)?;
                self.metrics
                    .auth_failures
                    .with_label_values(&["invalid"])
                    .inc();
                self.record_auth_denied(&request, "invalid bearer token");
                Err(Status::unauthenticated("invalid bearer token"))
            }
            None => {
                self.enforce_auth_failure_rate_limit(&request)?;
                self.metrics
                    .auth_failures
                    .with_label_values(&["missing"])
                    .inc();
                self.record_auth_denied(&request, "missing bearer token");
                Err(Status::unauthenticated("missing bearer token"))
            }
        }
    }
}

#[derive(Clone)]
struct DaemonMetrics {
    registry: Registry,
    request_latency: HistogramVec,
    auth_failures: IntCounterVec,
    policy_eval_duration: Histogram,
    queue_depth: IntGauge,
    worker_pool_size: IntGauge,
}

impl DaemonMetrics {
    fn new(worker_pool_size: usize) -> anyhow::Result<Self> {
        let registry = Registry::new();

        let request_latency = HistogramVec::new(
            HistogramOpts::new(
                "claw_daemon_http_request_latency_seconds",
                "HTTP request latency on daemon health listener",
            ),
            &["endpoint"],
        )?;
        let auth_failures = IntCounterVec::new(
            Opts::new(
                "claw_daemon_auth_failures_total",
                "Authentication failures by reason",
            ),
            &["reason"],
        )?;
        let policy_eval_duration = Histogram::with_opts(HistogramOpts::new(
            "claw_daemon_policy_eval_duration_seconds",
            "Duration of policy evaluations",
        ))?;
        let queue_depth = IntGauge::new("claw_daemon_queue_depth", "Current daemon queue depth")?;
        let worker_pool_size_gauge = IntGauge::new(
            "claw_daemon_worker_pool_size",
            "Configured daemon worker pool size",
        )?;

        registry.register(Box::new(request_latency.clone()))?;
        registry.register(Box::new(auth_failures.clone()))?;
        registry.register(Box::new(policy_eval_duration.clone()))?;
        registry.register(Box::new(queue_depth.clone()))?;
        registry.register(Box::new(worker_pool_size_gauge.clone()))?;

        request_latency.with_label_values(&["health_live"]);
        request_latency.with_label_values(&["health_ready"]);
        request_latency.with_label_values(&["health_deps"]);
        request_latency.with_label_values(&["metrics"]);
        request_latency.with_label_values(&["unknown"]);
        auth_failures.with_label_values(&["missing"]);
        auth_failures.with_label_values(&["invalid"]);
        auth_failures.with_label_values(&["throttled"]);

        queue_depth.set(0);
        worker_pool_size_gauge.set(worker_pool_size as i64);

        Ok(Self {
            registry,
            request_latency,
            auth_failures,
            policy_eval_duration,
            queue_depth,
            worker_pool_size: worker_pool_size_gauge,
        })
    }

    fn endpoint_label(path: &str) -> &'static str {
        match path {
            "/v1/health/live" => "health_live",
            "/v1/health/ready" => "health_ready",
            "/v1/health/deps" => "health_deps",
            "/v1/metrics" => "metrics",
            _ => "unknown",
        }
    }

    fn observe_http_latency(&self, path: &str, start: Instant) {
        let endpoint = Self::endpoint_label(path);
        self.request_latency
            .with_label_values(&[endpoint])
            .observe(start.elapsed().as_secs_f64());
    }

    fn render_prometheus(&self) -> anyhow::Result<Vec<u8>> {
        let metric_families = self.registry.gather();
        let mut out = Vec::new();
        TextEncoder::new().encode(&metric_families, &mut out)?;
        Ok(out)
    }

    fn register_metric_families(&self) {
        let _ = self.policy_eval_duration.get_sample_count();
        let _ = self.queue_depth.get();
        let _ = self.worker_pool_size.get();
    }
}

fn resolve_daemon_auth_token(args: &DaemonArgs) -> anyhow::Result<Option<String>> {
    if args.auth_token.is_some() && args.auth_profile.is_some() {
        anyhow::bail!("use either --auth-token or --auth-profile, not both");
    }

    if let Some(token) = &args.auth_token {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--auth-token cannot be empty");
        }
        return Ok(Some(trimmed.to_string()));
    }

    if let Some(profile) = &args.auth_profile {
        let token = auth_store::resolve_access_token(Some(profile)).ok_or_else(|| {
            anyhow::anyhow!(
                "no token for profile '{}'; run `claw auth login --profile {}`",
                profile,
                profile
            )
        })?;
        return Ok(Some(token));
    }

    Ok(None)
}

fn bearer_token_id(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("sha256:{}", &digest[..16])
}

fn validate_metadata_value(label: &str, value: &str) -> anyhow::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{label} cannot be empty");
    }
    MetadataValue::try_from(trimmed)
        .map_err(|err| anyhow::anyhow!("{label} must be valid ASCII gRPC metadata: {err}"))?;
    Ok(trimmed.to_string())
}

fn build_sync_authorizer(
    args: &DaemonArgs,
    auth_token: Option<&str>,
) -> anyhow::Result<Option<Arc<dyn Authorizer>>> {
    let authz_requested = auth_token.is_some()
        || !args.auth_role.eq_ignore_ascii_case("admin")
        || !args.auth_scopes.is_empty();
    if !authz_requested {
        return Ok(None);
    }

    let role = AuthorizationRole::from_str(&args.auth_role).map_err(anyhow::Error::msg)?;
    let scopes = args
        .auth_scopes
        .iter()
        .map(|scope| AuthorizationScope::from_str(scope).map_err(anyhow::Error::msg))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let principal = if auth_token.is_some() {
        validate_metadata_value("--auth-principal", &args.auth_principal)?
    } else {
        "anonymous".to_string()
    };

    let mut principals = vec![principal];
    if let Some(token) = auth_token {
        principals.push(bearer_token_id(token));
    }

    let mut authorizer = RoleBasedAuthorizer::new();
    for principal in principals {
        authorizer = authorizer.grant_role(principal.clone(), role);
        for scope in &scopes {
            authorizer = authorizer.grant_scope(principal.clone(), *scope);
        }
    }

    let authorizer: Arc<dyn Authorizer> = Arc::new(authorizer);
    Ok(Some(authorizer))
}

fn is_local_bind_addr(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

fn validate_distinct_listen_addrs(
    addr: &SocketAddr,
    health_addr: &SocketAddr,
) -> anyhow::Result<()> {
    if addr == health_addr {
        anyhow::bail!("--listen and --health-listen must use different addresses");
    }
    if addr.port() == health_addr.port()
        && (addr.ip() == health_addr.ip()
            || addr.ip().is_unspecified()
            || health_addr.ip().is_unspecified())
    {
        anyhow::bail!(
            "--listen and --health-listen must not share a port on overlapping bind interfaces"
        );
    }
    Ok(())
}

fn new_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{nanos:x}-{seq:x}")
}

#[derive(Serialize)]
struct HealthEnvelope<T: Serialize> {
    code: String,
    message: String,
    request_id: String,
    details: T,
}

#[derive(Serialize)]
struct HealthDetails {
    status: String,
}

fn health_response(
    request_id: String,
    message: &str,
    details: HealthDetails,
) -> HealthEnvelope<HealthDetails> {
    HealthEnvelope {
        code: "ok".to_string(),
        message: message.to_string(),
        request_id,
        details,
    }
}

fn health_live(request_id: String) -> HealthEnvelope<HealthDetails> {
    health_response(
        request_id,
        "daemon is live",
        HealthDetails {
            status: "live".to_string(),
        },
    )
}

fn health_ready(request_id: String) -> HealthEnvelope<HealthDetails> {
    health_response(
        request_id,
        "daemon is ready",
        HealthDetails {
            status: "ready".to_string(),
        },
    )
}

fn health_deps(request_id: String) -> HealthEnvelope<HealthDetails> {
    health_response(
        request_id,
        "dependencies are healthy",
        HealthDetails {
            status: "ok".to_string(),
        },
    )
}

fn extract_request_id(raw_request: &str) -> String {
    for line in raw_request.lines().skip(1) {
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("x-request-id") {
                let candidate = value.trim();
                if !candidate.is_empty() {
                    return candidate.to_string();
                }
            }
        }
    }
    new_request_id()
}

async fn write_http_json_response<T: Serialize>(
    stream: &mut tokio::net::TcpStream,
    status_line: &str,
    body: &T,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(body)?;
    let header = format!(
        "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await?;
    Ok(())
}

async fn write_http_text_response(
    stream: &mut tokio::net::TcpStream,
    status_line: &str,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 {status_line}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

fn log_health_request(request_id: &str, path: &str, status_code: u16) {
    tracing::info!(
        request_id = %request_id,
        path = %path,
        status_code,
        "health_http_request"
    );
}

#[derive(Debug, Deserialize)]
struct BackupManifest {
    files: Vec<BackupEntry>,
}

#[derive(Debug, Deserialize)]
struct BackupEntry {
    path: String,
    bytes: u64,
    sha256: String,
}

fn startup_verify_latest_backup(root: &Path, cfg: &config::ClawConfigV1) -> anyhow::Result<()> {
    if !cfg.backup.verify_integrity_on_startup {
        return Ok(());
    }

    let Some(backup_id) = latest_backup_id(root)? else {
        tracing::info!("startup backup integrity check enabled but no backups found");
        return Ok(());
    };

    match verify_backup_integrity(root, &backup_id) {
        Ok(()) => {
            tracing::info!(backup_id = %backup_id, "startup backup integrity check passed");
            Ok(())
        }
        Err(err) => {
            if cfg.backup.strict_startup_checks {
                anyhow::bail!(
                    "startup backup integrity check failed for {}: {}",
                    backup_id,
                    err
                );
            }
            tracing::warn!(
                backup_id = %backup_id,
                error = %err,
                "startup backup integrity check failed; continuing"
            );
            Ok(())
        }
    }
}

fn latest_backup_id(root: &Path) -> anyhow::Result<Option<String>> {
    let backups_dir = root.join(".claw").join("backups");
    if !backups_dir.is_dir() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(backups_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            candidates.push(entry.file_name().to_string_lossy().to_string());
        }
    }

    candidates.sort();
    Ok(candidates.pop())
}

fn verify_backup_integrity(root: &Path, backup_id: &str) -> anyhow::Result<()> {
    let base = root.join(".claw").join("backups").join(backup_id);
    let manifest_path = base.join("manifest.json");
    let snapshot_root = base.join("snapshot");
    let manifest: BackupManifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)
        .map_err(|err| {
            anyhow::anyhow!("invalid backup manifest {}: {err}", manifest_path.display())
        })?;

    for entry in manifest.files {
        let path = snapshot_root.join(&entry.path);
        let bytes = std::fs::read(&path)
            .map_err(|err| anyhow::anyhow!("backup file missing {}: {err}", path.display()))?;
        if bytes.len() as u64 != entry.bytes {
            anyhow::bail!("backup size mismatch for {}", entry.path);
        }

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let digest = hex::encode(hasher.finalize());
        if digest != entry.sha256 {
            anyhow::bail!("backup checksum mismatch for {}", entry.path);
        }
    }

    Ok(())
}

async fn handle_health_connection(
    mut stream: tokio::net::TcpStream,
    metrics: Arc<DaemonMetrics>,
) -> anyhow::Result<()> {
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buf[..n]);
    let request_id = extract_request_id(request.as_ref());
    let line = request.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let start = Instant::now();

    if method != "GET" {
        drain_request_body(&mut stream, &buf[..n]).await?;
        let envelope = HealthEnvelope {
            code: "method_not_allowed".to_string(),
            message: "method not allowed".to_string(),
            request_id: request_id.clone(),
            details: HealthDetails {
                status: "error".to_string(),
            },
        };
        write_http_json_response(&mut stream, "405 Method Not Allowed", &envelope).await?;
        log_health_request(&request_id, path, 405);
        metrics.observe_http_latency(path, start);
        return Ok(());
    }

    match path {
        "/v1/health/live" => {
            write_http_json_response(&mut stream, "200 OK", &health_live(request_id.clone()))
                .await?;
            log_health_request(&request_id, path, 200);
        }
        "/v1/health/ready" => {
            write_http_json_response(&mut stream, "200 OK", &health_ready(request_id.clone()))
                .await?;
            log_health_request(&request_id, path, 200);
        }
        "/v1/health/deps" => {
            write_http_json_response(&mut stream, "200 OK", &health_deps(request_id.clone()))
                .await?;
            log_health_request(&request_id, path, 200);
        }
        "/v1/metrics" => {
            metrics.register_metric_families();
            let payload = metrics.render_prometheus()?;
            write_http_text_response(
                &mut stream,
                "200 OK",
                "text/plain; version=0.0.4; charset=utf-8",
                &payload,
            )
            .await?;
            log_health_request(&request_id, path, 200);
        }
        _ => {
            let envelope = HealthEnvelope {
                code: "not_found".to_string(),
                message: "not found".to_string(),
                request_id: request_id.clone(),
                details: HealthDetails {
                    status: "error".to_string(),
                },
            };
            write_http_json_response(&mut stream, "404 Not Found", &envelope).await?;
            log_health_request(&request_id, path, 404);
        }
    }
    metrics.observe_http_latency(path, start);

    Ok(())
}

async fn drain_request_body(
    stream: &mut tokio::net::TcpStream,
    received: &[u8],
) -> anyhow::Result<()> {
    let Some(header_end) = received.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(());
    };
    let head = String::from_utf8_lossy(&received[..header_end]);
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if content_length > MAX_HEALTH_REQUEST_BODY_BYTES {
        anyhow::bail!(
            "health request body too large: {content_length} bytes exceeds {MAX_HEALTH_REQUEST_BODY_BYTES}"
        );
    }
    let already_read = received.len().saturating_sub(header_end + 4);
    let mut remaining = content_length.saturating_sub(already_read);
    let mut scratch = [0u8; 1024];

    while remaining > 0 {
        let to_read = remaining.min(scratch.len());
        let read = stream.read(&mut scratch[..to_read]).await?;
        if read == 0 {
            break;
        }
        remaining -= read;
    }

    Ok(())
}

async fn run_health_server(addr: SocketAddr, metrics: Arc<DaemonMetrics>) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let metrics = metrics.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_health_connection(stream, metrics).await {
                tracing::error!(error = %err, "health request failed");
            }
        });
    }
}

fn resolve_tls_identity(
    cert_path: Option<&Path>,
    key_path: Option<&Path>,
) -> anyhow::Result<Option<Identity>> {
    match (cert_path, key_path) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            anyhow::bail!("--tls-cert and --tls-key must be provided together")
        }
        (Some(cert_path), Some(key_path)) => {
            let cert = std::fs::read(cert_path)?;
            let key = std::fs::read(key_path)?;
            Ok(Some(Identity::from_pem(cert, key)))
        }
    }
}

fn resolve_client_ca_certificate(cert_path: Option<&Path>) -> anyhow::Result<Option<Certificate>> {
    let Some(cert_path) = cert_path else {
        return Ok(None);
    };
    let cert = std::fs::read(cert_path)?;
    Ok(Some(Certificate::from_pem(cert)))
}

fn build_audit_sink(path: Option<&Path>) -> anyhow::Result<Arc<dyn AuditSink>> {
    let tracing_sink: Arc<dyn AuditSink> = Arc::new(TracingAuditSink);
    let Some(path) = path else {
        return Ok(tracing_sink);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_sink: Arc<dyn AuditSink> = Arc::new(JsonlAuditSink::open(path)?);
    Ok(Arc::new(TeeAuditSink::new(tracing_sink, file_sink)))
}

fn grpc_message_limit_bytes(options: &SyncServerOptions) -> usize {
    options
        .max_push_chunk_bytes
        .unwrap_or_else(|| {
            SyncServerOptions::default()
                .max_push_chunk_bytes
                .unwrap_or(8 * 1024 * 1024)
        })
        .saturating_add(GRPC_MESSAGE_OVERHEAD_BYTES)
}

pub async fn run(args: DaemonArgs, runtime: &RuntimeOptions) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let cfg = config::load_or_default_config(&root)?;
    startup_verify_latest_backup(&root, &cfg)?;
    let store = ClawStore::open(&root)?;
    let mut auth_token = resolve_daemon_auth_token(&args)?;

    if args.stdio {
        // Stdio mode: read/write framed messages on stdin/stdout.
        // For embedded agent use. Uses length-prefixed JSON frames.
        eprintln!("Claw daemon running in stdio mode");
        eprintln!("Send JSON-RPC requests on stdin, receive responses on stdout");

        use tokio::io::{AsyncBufReadExt, BufReader};
        let stdin = BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();

        while let Some(line) = lines.next_line().await? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            // Stdio clients use newline-delimited JSON requests for embedded
            // agent integrations. Keep responses structured so callers can
            // negotiate capabilities before issuing ref or sync requests.
            let response = match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(req) => {
                    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    match method {
                        "hello" => serde_json::json!({
                            "server_version": env!("CARGO_PKG_VERSION"),
                            "protocol_version": SYNC_PROTOCOL_VERSION,
                            "capabilities": server_capabilities()
                        }),
                        "refs" => {
                            let prefix = req.get("prefix").and_then(|p| p.as_str()).unwrap_or("");
                            match store.list_refs(prefix) {
                                Ok(refs) => {
                                    let r: Vec<_> = refs.iter()
                                        .map(|(name, id)| serde_json::json!({"name": name, "target": id.to_string()}))
                                        .collect();
                                    serde_json::json!({"refs": r})
                                }
                                Err(e) => serde_json::json!({"error": e.to_string()}),
                            }
                        }
                        _ => serde_json::json!({"error": format!("unknown method: {method}")}),
                    }
                }
                Err(e) => serde_json::json!({"error": e.to_string()}),
            };
            println!("{}", serde_json::to_string(&response)?);
        }
        return Ok(());
    }

    let addr: SocketAddr = args.listen.parse()?;
    let health_addr: SocketAddr = args.health_listen.parse()?;
    validate_distinct_listen_addrs(&addr, &health_addr)?;
    let non_local_bind = !is_local_bind_addr(&addr);
    let non_local_health_bind = !is_local_bind_addr(&health_addr);

    if auth_token.is_none() {
        let profile = config::default_profile(&cfg);
        auth_token = auth_store::resolve_access_token(Some(profile));
    }

    let tls_cert = args
        .tls_cert
        .clone()
        .or_else(|| cfg.tls.cert_path.as_ref().map(PathBuf::from));
    let tls_key = args
        .tls_key
        .clone()
        .or_else(|| cfg.tls.key_path.as_ref().map(PathBuf::from));
    let tls_identity = resolve_tls_identity(tls_cert.as_deref(), tls_key.as_deref())?;
    let client_ca_certificate = resolve_client_ca_certificate(args.client_ca_cert.as_deref())?;
    let mtls_enabled = client_ca_certificate.is_some();
    if mtls_enabled && tls_identity.is_none() {
        anyhow::bail!("--client-ca-cert requires --tls-cert and --tls-key");
    }
    let sync_authorizer = build_sync_authorizer(&args, auth_token.as_deref())?;

    let enforce_prod_profile = runtime.profile.eq_ignore_ascii_case("prod");
    if non_local_bind && enforce_prod_profile {
        if cfg.auth.require_auth_for_daemon && auth_token.is_none() {
            anyhow::bail!(
                "non-local bind requires authentication; use --auth-token, --auth-profile, or set a token for default profile"
            );
        }
        if cfg.tls.require_for_non_localhost && tls_identity.is_none() {
            anyhow::bail!(
                "non-local bind requires TLS; provide --tls-cert/--tls-key or set tls.cert_path/tls.key_path in .claw/config.toml"
            );
        }
    }
    if non_local_health_bind && enforce_prod_profile && !args.allow_public_health {
        anyhow::bail!(
            "non-local health bind exposes unauthenticated health and metrics; use --allow-public-health to opt in"
        );
    }

    let shared_store = Arc::new(RwLock::new(store));
    let metrics = Arc::new(DaemonMetrics::new(cfg.queues.worker_pool_size)?);
    let event_bus = EventBus::default();
    let sync_defaults = SyncServerOptions::default();

    let sync_options = SyncServerOptions {
        worker_pool_size: cfg.queues.worker_pool_size,
        queue_capacity: cfg.queues.queue_capacity,
        backpressure: cfg.queues.backpressure,
        io_timeout: std::time::Duration::from_millis(cfg.timeouts.io_ms),
        max_push_chunk_bytes: args
            .max_push_chunk_bytes
            .or(cfg.queues.max_push_chunk_bytes)
            .or(sync_defaults.max_push_chunk_bytes),
        max_push_request_bytes: args
            .max_push_request_bytes
            .or(cfg.queues.max_push_request_bytes)
            .or(sync_defaults.max_push_request_bytes),
        rate_limit_per_minute: args
            .rate_limit_per_minute
            .or(cfg.queues.rate_limit_per_minute)
            .or(sync_defaults.rate_limit_per_minute),
    };
    let auth_failure_rate_limit = sync_options.rate_limit_per_minute;
    let grpc_message_limit = grpc_message_limit_bytes(&sync_options);
    let mut sync_server = SyncServer::from_shared_with_options_and_events(
        shared_store.clone(),
        sync_options,
        Some(event_bus.clone()),
    )
    .with_replay_protection(ReplayProtectionConfig::default(), args.require_replay_nonce);
    if let Some(authorizer) = sync_authorizer.as_ref() {
        sync_server = sync_server.with_authorizer(authorizer.clone());
    }
    let mut intent_server = IntentServer::new(shared_store.clone());
    let mut change_server = ChangeServer::new(shared_store.clone());
    let mut capsule_server = CapsuleServer::new(shared_store.clone());
    let mut workstream_server = WorkstreamServer::new(shared_store.clone());
    let mut event_server = EventServer::with_bus(event_bus);
    let audit_sink = build_audit_sink(args.audit_log.as_deref())?;
    sync_server = sync_server.with_audit_sink(audit_sink.clone());
    intent_server = intent_server.with_audit_sink(audit_sink.clone());
    change_server = change_server.with_audit_sink(audit_sink.clone());
    capsule_server = capsule_server.with_audit_sink(audit_sink.clone());
    workstream_server = workstream_server.with_audit_sink(audit_sink.clone());
    event_server = event_server.with_audit_sink(audit_sink.clone());
    if let Some(authorizer) = sync_authorizer {
        intent_server = intent_server.with_authorizer(authorizer.clone());
        change_server = change_server.with_authorizer(authorizer.clone());
        capsule_server = capsule_server.with_authorizer(authorizer.clone());
        workstream_server = workstream_server.with_authorizer(authorizer.clone());
        event_server = event_server.with_authorizer(authorizer);
    }

    let mut grpc_builder = Server::builder();
    if let Some(identity) = tls_identity {
        let mut tls_config = ServerTlsConfig::new().identity(identity);
        if let Some(client_ca) = client_ca_certificate {
            tls_config = tls_config.client_ca_root(client_ca);
        }
        grpc_builder = grpc_builder.tls_config(tls_config)?;
    }

    println!("Claw daemon listening (gRPC) on {}", addr);
    println!("Claw daemon listening (health) on {}", health_addr);
    println!("Runtime profile: {}", runtime.profile);
    if auth_token.is_some() {
        println!("gRPC auth enabled (bearer token required)");
    }
    if mtls_enabled {
        println!("gRPC mTLS enabled (client certificate required)");
    }
    if args.require_replay_nonce {
        println!("sync replay nonce enforcement enabled");
    }

    let grpc_metrics = metrics.clone();
    let auth_principal = validate_metadata_value("--auth-principal", &args.auth_principal)?;
    let grpc_task = async move {
        if let Some(token) = auth_token {
            let interceptor = BearerAuthInterceptor::new(
                token,
                auth_principal,
                grpc_metrics.clone(),
                audit_sink.clone(),
            )
            .with_auth_failure_rate_limit(auth_failure_rate_limit);
            grpc_builder
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    SyncServiceServer::new(sync_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                    interceptor.clone(),
                ))
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    IntentServiceServer::new(intent_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                    interceptor.clone(),
                ))
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    ChangeServiceServer::new(change_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                    interceptor.clone(),
                ))
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    CapsuleServiceServer::new(capsule_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                    interceptor.clone(),
                ))
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    WorkstreamServiceServer::new(workstream_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                    interceptor.clone(),
                ))
                .add_service(tonic::service::interceptor::InterceptedService::new(
                    EventStreamServiceServer::new(event_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                    interceptor,
                ))
                .serve(addr)
                .await?;
        } else {
            grpc_builder
                .add_service(
                    SyncServiceServer::new(sync_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                )
                .add_service(
                    IntentServiceServer::new(intent_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                )
                .add_service(
                    ChangeServiceServer::new(change_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                )
                .add_service(
                    CapsuleServiceServer::new(capsule_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                )
                .add_service(
                    WorkstreamServiceServer::new(workstream_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                )
                .add_service(
                    EventStreamServiceServer::new(event_server)
                        .max_decoding_message_size(grpc_message_limit)
                        .max_encoding_message_size(grpc_message_limit),
                )
                .serve(addr)
                .await?;
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::try_join!(grpc_task, run_health_server(health_addr, metrics))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;
    use tonic::service::Interceptor;

    #[derive(Clone, Default)]
    struct TestAuditSink {
        events: Arc<Mutex<Vec<AuditEvent>>>,
    }

    impl AuditSink for TestAuditSink {
        fn record(&self, event: AuditEvent) {
            self.events.lock().expect("audit lock").push(event);
        }
    }

    fn test_metrics() -> Arc<DaemonMetrics> {
        Arc::new(DaemonMetrics::new(8).expect("create test metrics"))
    }

    async fn exercise_health_handler_raw(
        raw_request: &str,
        metrics: Arc<DaemonMetrics>,
    ) -> (String, String, String) {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind local health test listener");
        let addr = listener.local_addr().expect("read local listener address");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept test connection");
            handle_health_connection(stream, metrics).await
        });

        let mut client = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to local health test listener");
        client
            .write_all(raw_request.as_bytes())
            .await
            .expect("write test request");
        client.shutdown().await.expect("shutdown write half");

        let mut response_bytes = Vec::new();
        client
            .read_to_end(&mut response_bytes)
            .await
            .expect("read full health response");

        server
            .await
            .expect("join health handler task")
            .expect("health handler succeeds");

        let response = String::from_utf8(response_bytes).expect("response is utf-8");
        let (head, body) = response
            .split_once("\r\n\r\n")
            .expect("response contains header/body separator");
        let status_line = head.lines().next().unwrap_or_default().to_string();
        (status_line, head.to_string(), body.to_string())
    }

    async fn exercise_health_handler(raw_request: &str) -> (String, Value) {
        let (status_line, _head, body) =
            exercise_health_handler_raw(raw_request, test_metrics()).await;
        let json = serde_json::from_str::<Value>(&body).expect("response body is valid json");

        (status_line, json)
    }

    #[test]
    fn localhost_bind_is_classified_as_local() {
        let ipv4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50051);
        let ipv6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 50051);

        assert!(is_local_bind_addr(&ipv4));
        assert!(is_local_bind_addr(&ipv6));
    }

    #[test]
    fn non_local_bind_is_not_classified_as_local() {
        let non_local = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 50051);

        assert!(!is_local_bind_addr(&non_local));
    }

    #[test]
    fn daemon_listen_addresses_must_be_distinct() {
        let grpc = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50051);
        let same_health = grpc;
        let wildcard_same_port = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 50051);
        let distinct_health = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50052);

        let err = validate_distinct_listen_addrs(&grpc, &same_health)
            .expect_err("same daemon listener addresses should fail");
        assert!(err
            .to_string()
            .contains("--listen and --health-listen must use different addresses"));
        let err = validate_distinct_listen_addrs(&grpc, &wildcard_same_port)
            .expect_err("overlapping daemon listener ports should fail");
        assert!(err.to_string().contains(
            "--listen and --health-listen must not share a port on overlapping bind interfaces"
        ));
        validate_distinct_listen_addrs(&grpc, &distinct_health)
            .expect("distinct daemon listener addresses should pass");
    }

    #[test]
    fn request_id_helper_generates_prefixed_unique_ids() {
        let first = new_request_id();
        let second = new_request_id();

        assert!(first.starts_with("req-"));
        assert!(second.starts_with("req-"));
        assert_ne!(first, second);
    }

    #[test]
    fn extract_request_id_uses_header_when_present() {
        let req =
            "GET /v1/health/live HTTP/1.1\r\nHost: localhost\r\nX-Request-Id: abc-123\r\n\r\n";
        assert_eq!(extract_request_id(req), "abc-123");
    }

    #[test]
    fn extract_request_id_generates_when_missing() {
        let req = "GET /v1/health/live HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let id = extract_request_id(req);
        assert!(id.starts_with("req-"));
    }

    #[tokio::test]
    async fn health_live_returns_200_with_json_envelope() {
        let request = "GET /v1/health/live HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status_line, body) = exercise_health_handler(request).await;

        assert_eq!(status_line, "HTTP/1.1 200 OK");
        assert!(body.get("code").is_some());
        assert!(body.get("message").is_some());
        assert!(body.get("request_id").is_some());
        assert!(body.get("details").is_some());
    }

    #[tokio::test]
    async fn health_unknown_path_returns_404_envelope() {
        let request = "GET /v1/health/unknown HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status_line, body) = exercise_health_handler(request).await;

        assert_eq!(status_line, "HTTP/1.1 404 Not Found");
        assert_eq!(body.get("code").and_then(Value::as_str), Some("not_found"));
        assert!(body.get("request_id").is_some());
    }

    #[tokio::test]
    async fn health_echoes_x_request_id_into_envelope() {
        let request =
            "GET /v1/health/live HTTP/1.1\r\nHost: localhost\r\nX-Request-Id: req-test-123\r\n\r\n";
        let (status_line, body) = exercise_health_handler(request).await;

        assert_eq!(status_line, "HTTP/1.1 200 OK");
        assert_eq!(
            body.get("request_id").and_then(Value::as_str),
            Some("req-test-123")
        );
    }

    #[tokio::test]
    async fn metrics_endpoint_returns_prometheus_text_shape() {
        let request = "GET /v1/metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status_line, head, body) = exercise_health_handler_raw(request, test_metrics()).await;

        assert_eq!(status_line, "HTTP/1.1 200 OK");
        assert!(head.contains("content-type: text/plain; version=0.0.4; charset=utf-8"));
        assert!(body.contains("# HELP claw_daemon_http_request_latency_seconds"));
        assert!(body.contains("# HELP claw_daemon_auth_failures_total"));
        assert!(body.contains("# HELP claw_daemon_policy_eval_duration_seconds"));
        assert!(body.contains("# HELP claw_daemon_queue_depth"));
        assert!(body.contains("# HELP claw_daemon_worker_pool_size"));
    }

    #[tokio::test]
    async fn drain_request_body_rejects_large_content_length() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind local body-drain test listener");
        let addr = listener.local_addr().expect("read local listener address");
        let client = tokio::spawn(async move {
            tokio::net::TcpStream::connect(addr)
                .await
                .expect("connect to local body-drain listener")
        });
        let (mut stream, _) = listener.accept().await.expect("accept test connection");
        let _client = client.await.expect("join client task");
        let request = format!(
            "POST /v1/health/live HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            MAX_HEALTH_REQUEST_BODY_BYTES + 1
        );

        let err = drain_request_body(&mut stream, request.as_bytes())
            .await
            .expect_err("oversized health body should be rejected");

        assert!(err.to_string().contains("health request body too large"));
    }

    #[test]
    fn auth_failure_metrics_increment_for_missing_and_invalid_bearer() {
        let metrics = test_metrics();
        let audit = TestAuditSink::default();
        let mut interceptor = BearerAuthInterceptor::new(
            "correct-token".to_string(),
            "agent-a".to_string(),
            metrics.clone(),
            Arc::new(audit.clone()),
        );

        let missing = interceptor.call(Request::new(()));
        assert!(missing.is_err());

        let mut invalid_req = Request::new(());
        invalid_req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer wrong-token").expect("valid metadata value"),
        );
        let invalid = interceptor.call(invalid_req);
        assert!(invalid.is_err());

        assert_eq!(
            metrics.auth_failures.with_label_values(&["missing"]).get(),
            1
        );
        assert_eq!(
            metrics.auth_failures.with_label_values(&["invalid"]).get(),
            1
        );
        let events = audit.events.lock().expect("audit lock");
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|event| event.outcome == claw_sync::security::AuditOutcome::Denied));
        assert!(events
            .iter()
            .any(|event| event.reason.as_deref() == Some("missing bearer token")));
        assert!(events
            .iter()
            .any(|event| event.reason.as_deref() == Some("invalid bearer token")));
    }

    #[test]
    fn auth_failure_rate_limit_throttles_invalid_bearer_floods() {
        let metrics = test_metrics();
        let audit = TestAuditSink::default();
        let mut interceptor = BearerAuthInterceptor::new(
            "correct-token".to_string(),
            "agent-a".to_string(),
            metrics.clone(),
            Arc::new(audit.clone()),
        )
        .with_auth_failure_rate_limit(Some(1));

        let mut first = Request::new(());
        first.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer wrong-token-1").expect("valid metadata value"),
        );
        assert!(interceptor.call(first).is_err());

        let mut second = Request::new(());
        second.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer wrong-token-2").expect("valid metadata value"),
        );
        let throttled = interceptor
            .call(second)
            .expect_err("second auth failure should be throttled");

        assert_eq!(throttled.code(), tonic::Code::ResourceExhausted);
        assert_eq!(
            metrics.auth_failures.with_label_values(&["invalid"]).get(),
            1
        );
        assert_eq!(
            metrics
                .auth_failures
                .with_label_values(&["throttled"])
                .get(),
            1
        );
        assert!(audit
            .events
            .lock()
            .expect("audit lock")
            .iter()
            .any(|event| event.reason.as_deref() == Some("auth failure rate limit exceeded")));
    }

    #[test]
    fn grpc_message_limit_tracks_configured_push_chunk_limit() {
        let options = SyncServerOptions {
            max_push_chunk_bytes: Some(16 * 1024 * 1024),
            ..SyncServerOptions::default()
        };

        assert_eq!(
            grpc_message_limit_bytes(&options),
            17 * 1024 * 1024,
            "tonic message limits should leave headroom above Claw's push chunk limit"
        );
    }

    #[test]
    fn tls_identity_requires_cert_and_key_pair() {
        let cert_only = resolve_tls_identity(Some(Path::new("server.pem")), None)
            .expect_err("missing key should fail");
        assert!(cert_only.to_string().contains("--tls-cert and --tls-key"));

        let key_only = resolve_tls_identity(None, Some(Path::new("server-key.pem")))
            .expect_err("missing cert should fail");
        assert!(key_only.to_string().contains("--tls-cert and --tls-key"));
    }

    #[test]
    fn client_ca_certificate_loader_reads_configured_pem() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ca_path = temp.path().join("ca.pem");
        std::fs::write(
            &ca_path,
            b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n",
        )
        .expect("write ca");

        let cert =
            resolve_client_ca_certificate(Some(&ca_path)).expect("client CA PEM should be read");
        assert!(cert.is_some());
    }

    #[test]
    fn bearer_auth_interceptor_attaches_authorization_subject_metadata() {
        let metrics = test_metrics();
        let mut interceptor = BearerAuthInterceptor::new(
            "correct-token".to_string(),
            "agent-a".to_string(),
            metrics,
            Arc::new(TracingAuditSink),
        );

        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer correct-token").expect("valid metadata value"),
        );

        let request = interceptor.call(request).expect("valid bearer token");
        let expected_token_id = bearer_token_id("correct-token");
        assert_eq!(
            request
                .metadata()
                .get(PRINCIPAL_METADATA_KEY)
                .and_then(|value| value.to_str().ok()),
            Some("agent-a")
        );
        assert_eq!(
            request
                .metadata()
                .get(TOKEN_ID_METADATA_KEY)
                .and_then(|value| value.to_str().ok()),
            Some(expected_token_id.as_str())
        );
    }

    #[test]
    fn sync_authorizer_grants_configured_role_and_scopes_to_token_principal() {
        let args = DaemonArgs {
            listen: "[::1]:50051".to_string(),
            health_listen: "[::1]:50052".to_string(),
            stdio: false,
            auth_token: Some("correct-token".to_string()),
            auth_profile: None,
            auth_principal: "agent-a".to_string(),
            auth_role: "reader".to_string(),
            auth_scopes: vec!["refs:write".to_string()],
            require_replay_nonce: false,
            rate_limit_per_minute: None,
            max_push_chunk_bytes: None,
            max_push_request_bytes: None,
            tls_cert: None,
            tls_key: None,
            client_ca_cert: None,
            audit_log: None,
            allow_public_health: false,
        };

        let authorizer = build_sync_authorizer(&args, args.auth_token.as_deref())
            .expect("valid authorizer")
            .expect("auth token enables role authorizer");
        let subject = claw_sync::security::AuthorizationSubject {
            principal: Some("agent-a".to_string()),
            token_id: Some(bearer_token_id("correct-token")),
            peer_addr: None,
        };

        assert!(authorizer
            .authorize(&claw_sync::security::AuthorizationRequest {
                subject,
                action: claw_sync::security::AuthorizationAction::UpdateRefs,
                resource: Some("heads/main".to_string()),
            })
            .is_allowed());
    }
}
