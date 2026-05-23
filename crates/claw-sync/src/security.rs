use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::{Request, Status};

pub const REQUEST_ID_METADATA_KEY: &str = "x-request-id";
pub const PRINCIPAL_METADATA_KEY: &str = "x-claw-principal";
pub const TOKEN_ID_METADATA_KEY: &str = "x-claw-token-id";
pub const REPLAY_NONCE_METADATA_KEY: &str = "x-claw-replay-nonce";

static SERVICE_REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationAction {
    Hello,
    AdvertiseRefs,
    FetchObjects,
    PushObjects,
    UpdateRefs,
    SubscribeEvents,
    CreateIntent,
    ReadIntent,
    UpdateIntent,
    CreateChange,
    ReadChange,
    UpdateChange,
    CreateCapsule,
    ReadCapsule,
    ReadPrivateCapsule,
    VerifyCapsule,
    CreateWorkstream,
    ReadWorkstream,
    UpdateWorkstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorizationScope {
    SyncHello,
    SyncAll,
    RefsRead,
    RefsWrite,
    RefsAll,
    ObjectsRead,
    ObjectsWrite,
    ObjectsAll,
    EventsRead,
    EventsAll,
    IntentsRead,
    IntentsWrite,
    IntentsAll,
    ChangesRead,
    ChangesWrite,
    ChangesAll,
    CapsulesRead,
    CapsulesWrite,
    CapsulesVerify,
    CapsulesPrivateRead,
    CapsulesAll,
    WorkstreamsRead,
    WorkstreamsWrite,
    WorkstreamsAll,
}

impl AuthorizationScope {
    pub fn required_for(action: &AuthorizationAction) -> Self {
        match action {
            AuthorizationAction::Hello => Self::SyncHello,
            AuthorizationAction::AdvertiseRefs => Self::RefsRead,
            AuthorizationAction::FetchObjects => Self::ObjectsRead,
            AuthorizationAction::PushObjects => Self::ObjectsWrite,
            AuthorizationAction::UpdateRefs => Self::RefsWrite,
            AuthorizationAction::SubscribeEvents => Self::EventsRead,
            AuthorizationAction::CreateIntent => Self::IntentsWrite,
            AuthorizationAction::ReadIntent => Self::IntentsRead,
            AuthorizationAction::UpdateIntent => Self::IntentsWrite,
            AuthorizationAction::CreateChange => Self::ChangesWrite,
            AuthorizationAction::ReadChange => Self::ChangesRead,
            AuthorizationAction::UpdateChange => Self::ChangesWrite,
            AuthorizationAction::CreateCapsule => Self::CapsulesWrite,
            AuthorizationAction::ReadCapsule => Self::CapsulesRead,
            AuthorizationAction::ReadPrivateCapsule => Self::CapsulesPrivateRead,
            AuthorizationAction::VerifyCapsule => Self::CapsulesVerify,
            AuthorizationAction::CreateWorkstream => Self::WorkstreamsWrite,
            AuthorizationAction::ReadWorkstream => Self::WorkstreamsRead,
            AuthorizationAction::UpdateWorkstream => Self::WorkstreamsWrite,
        }
    }

    fn grants(self, required: Self) -> bool {
        self == required
            || matches!(self, Self::SyncAll)
            || matches!(
                (self, required),
                (Self::RefsAll, Self::RefsRead | Self::RefsWrite)
                    | (Self::ObjectsAll, Self::ObjectsRead | Self::ObjectsWrite)
                    | (Self::EventsAll, Self::EventsRead)
                    | (Self::IntentsAll, Self::IntentsRead | Self::IntentsWrite)
                    | (Self::ChangesAll, Self::ChangesRead | Self::ChangesWrite)
                    | (
                        Self::CapsulesAll,
                        Self::CapsulesRead
                            | Self::CapsulesWrite
                            | Self::CapsulesVerify
                            | Self::CapsulesPrivateRead
                    )
                    | (
                        Self::WorkstreamsAll,
                        Self::WorkstreamsRead | Self::WorkstreamsWrite
                    )
            )
    }
}

impl std::fmt::Display for AuthorizationScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SyncHello => "sync:hello",
            Self::SyncAll => "sync:*",
            Self::RefsRead => "refs:read",
            Self::RefsWrite => "refs:write",
            Self::RefsAll => "refs:*",
            Self::ObjectsRead => "objects:read",
            Self::ObjectsWrite => "objects:write",
            Self::ObjectsAll => "objects:*",
            Self::EventsRead => "events:read",
            Self::EventsAll => "events:*",
            Self::IntentsRead => "intents:read",
            Self::IntentsWrite => "intents:write",
            Self::IntentsAll => "intents:*",
            Self::ChangesRead => "changes:read",
            Self::ChangesWrite => "changes:write",
            Self::ChangesAll => "changes:*",
            Self::CapsulesRead => "capsules:read",
            Self::CapsulesWrite => "capsules:write",
            Self::CapsulesVerify => "capsules:verify",
            Self::CapsulesPrivateRead => "capsules:private-read",
            Self::CapsulesAll => "capsules:*",
            Self::WorkstreamsRead => "workstreams:read",
            Self::WorkstreamsWrite => "workstreams:write",
            Self::WorkstreamsAll => "workstreams:*",
        })
    }
}

impl FromStr for AuthorizationScope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sync:hello" | "hello" => Ok(Self::SyncHello),
            "sync:*" | "sync:all" | "admin" => Ok(Self::SyncAll),
            "refs:read" | "ref:read" => Ok(Self::RefsRead),
            "refs:write" | "ref:write" => Ok(Self::RefsWrite),
            "refs:*" | "refs:all" | "ref:*" | "ref:all" => Ok(Self::RefsAll),
            "objects:read" | "object:read" => Ok(Self::ObjectsRead),
            "objects:write" | "object:write" => Ok(Self::ObjectsWrite),
            "objects:*" | "objects:all" | "object:*" | "object:all" => Ok(Self::ObjectsAll),
            "events:read" | "event:read" => Ok(Self::EventsRead),
            "events:*" | "events:all" | "event:*" | "event:all" => Ok(Self::EventsAll),
            "intents:read" | "intent:read" => Ok(Self::IntentsRead),
            "intents:write" | "intent:write" => Ok(Self::IntentsWrite),
            "intents:*" | "intents:all" | "intent:*" | "intent:all" => Ok(Self::IntentsAll),
            "changes:read" | "change:read" => Ok(Self::ChangesRead),
            "changes:write" | "change:write" => Ok(Self::ChangesWrite),
            "changes:*" | "changes:all" | "change:*" | "change:all" => Ok(Self::ChangesAll),
            "capsules:read" | "capsule:read" => Ok(Self::CapsulesRead),
            "capsules:write" | "capsule:write" => Ok(Self::CapsulesWrite),
            "capsules:verify" | "capsule:verify" => Ok(Self::CapsulesVerify),
            "capsules:private-read" | "capsules:private:read" | "capsule:private-read" => {
                Ok(Self::CapsulesPrivateRead)
            }
            "capsules:*" | "capsules:all" | "capsule:*" | "capsule:all" => Ok(Self::CapsulesAll),
            "workstreams:read" | "workstream:read" => Ok(Self::WorkstreamsRead),
            "workstreams:write" | "workstream:write" => Ok(Self::WorkstreamsWrite),
            "workstreams:*" | "workstreams:all" | "workstream:*" | "workstream:all" => {
                Ok(Self::WorkstreamsAll)
            }
            other => Err(format!("unknown authorization scope '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorizationRole {
    Reader,
    ObjectWriter,
    RefWriter,
    Writer,
    EventReader,
    Admin,
}

impl AuthorizationRole {
    pub fn scopes(self) -> &'static [AuthorizationScope] {
        match self {
            Self::Reader => &[
                AuthorizationScope::SyncHello,
                AuthorizationScope::RefsRead,
                AuthorizationScope::ObjectsRead,
                AuthorizationScope::EventsRead,
                AuthorizationScope::IntentsRead,
                AuthorizationScope::ChangesRead,
                AuthorizationScope::CapsulesRead,
                AuthorizationScope::CapsulesVerify,
                AuthorizationScope::WorkstreamsRead,
            ],
            Self::ObjectWriter => &[
                AuthorizationScope::SyncHello,
                AuthorizationScope::RefsRead,
                AuthorizationScope::ObjectsRead,
                AuthorizationScope::ObjectsWrite,
            ],
            Self::RefWriter => &[
                AuthorizationScope::SyncHello,
                AuthorizationScope::RefsRead,
                AuthorizationScope::ObjectsRead,
                AuthorizationScope::RefsWrite,
            ],
            Self::Writer => &[
                AuthorizationScope::SyncHello,
                AuthorizationScope::RefsRead,
                AuthorizationScope::ObjectsRead,
                AuthorizationScope::ObjectsWrite,
                AuthorizationScope::RefsWrite,
                AuthorizationScope::EventsRead,
                AuthorizationScope::IntentsAll,
                AuthorizationScope::ChangesAll,
                AuthorizationScope::CapsulesAll,
                AuthorizationScope::WorkstreamsAll,
            ],
            Self::EventReader => &[
                AuthorizationScope::SyncHello,
                AuthorizationScope::EventsRead,
            ],
            Self::Admin => &[AuthorizationScope::SyncAll],
        }
    }
}

impl std::fmt::Display for AuthorizationRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Reader => "reader",
            Self::ObjectWriter => "object-writer",
            Self::RefWriter => "ref-writer",
            Self::Writer => "writer",
            Self::EventReader => "event-reader",
            Self::Admin => "admin",
        })
    }
}

impl FromStr for AuthorizationRole {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "reader" | "read" => Ok(Self::Reader),
            "object-writer" | "objects-writer" | "object_writer" => Ok(Self::ObjectWriter),
            "ref-writer" | "refs-writer" | "ref_writer" => Ok(Self::RefWriter),
            "writer" | "write" => Ok(Self::Writer),
            "event-reader" | "events-reader" | "event_reader" => Ok(Self::EventReader),
            "admin" | "administrator" => Ok(Self::Admin),
            other => Err(format!("unknown authorization role '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationSubject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_addr: Option<String>,
}

impl AuthorizationSubject {
    pub fn anonymous() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub subject: AuthorizationSubject,
    pub action: AuthorizationAction,
    pub resource: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationDecision {
    Allow,
    Deny { reason: String },
}

impl AuthorizationDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

pub trait Authorizer: Send + Sync {
    fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationDecision;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllAuthorizer;

impl Authorizer for AllowAllAuthorizer {
    fn authorize(&self, _request: &AuthorizationRequest) -> AuthorizationDecision {
        AuthorizationDecision::Allow
    }
}

#[derive(Debug, Clone, Default)]
pub struct PrincipalGrant {
    roles: HashSet<AuthorizationRole>,
    scopes: HashSet<AuthorizationScope>,
}

impl PrincipalGrant {
    pub fn with_role(mut self, role: AuthorizationRole) -> Self {
        self.roles.insert(role);
        self
    }

    pub fn with_scope(mut self, scope: AuthorizationScope) -> Self {
        self.scopes.insert(scope);
        self
    }

    fn allows(&self, required: AuthorizationScope) -> bool {
        self.scopes.iter().any(|scope| scope.grants(required))
            || self
                .roles
                .iter()
                .flat_map(|role| role.scopes())
                .any(|scope| scope.grants(required))
    }
}

#[derive(Debug, Clone, Default)]
pub struct RoleBasedAuthorizer {
    grants: HashMap<String, PrincipalGrant>,
}

impl RoleBasedAuthorizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant_role(mut self, principal: impl Into<String>, role: AuthorizationRole) -> Self {
        self.grants
            .entry(principal.into())
            .or_default()
            .roles
            .insert(role);
        self
    }

    pub fn grant_scope(mut self, principal: impl Into<String>, scope: AuthorizationScope) -> Self {
        self.grants
            .entry(principal.into())
            .or_default()
            .scopes
            .insert(scope);
        self
    }

    fn grant_for_subject(&self, subject: &AuthorizationSubject) -> Option<&PrincipalGrant> {
        subject
            .principal
            .as_deref()
            .and_then(|principal| self.grants.get(principal))
            .or_else(|| {
                subject
                    .token_id
                    .as_deref()
                    .and_then(|token_id| self.grants.get(token_id))
            })
            .or_else(|| self.grants.get("anonymous"))
    }
}

impl Authorizer for RoleBasedAuthorizer {
    fn authorize(&self, request: &AuthorizationRequest) -> AuthorizationDecision {
        let required = AuthorizationScope::required_for(&request.action);
        let Some(grant) = self.grant_for_subject(&request.subject) else {
            return AuthorizationDecision::Deny {
                reason: "subject has no sync authorization grant".to_string(),
            };
        };

        if grant.allows(required) {
            AuthorizationDecision::Allow
        } else {
            AuthorizationDecision::Deny {
                reason: format!("missing required scope {required}"),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Allowed,
    Denied,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp_ms: u64,
    pub request_id: String,
    pub subject: AuthorizationSubject,
    pub action: AuthorizationAction,
    pub outcome: AuditOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl AuditEvent {
    pub fn allowed(
        timestamp_ms: u64,
        request_id: impl Into<String>,
        subject: AuthorizationSubject,
        action: AuthorizationAction,
        resource: Option<String>,
    ) -> Self {
        Self {
            timestamp_ms,
            request_id: request_id.into(),
            subject,
            action,
            outcome: AuditOutcome::Allowed,
            resource,
            reason: None,
        }
    }

    pub fn denied(
        timestamp_ms: u64,
        request_id: impl Into<String>,
        subject: AuthorizationSubject,
        action: AuthorizationAction,
        resource: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_ms,
            request_id: request_id.into(),
            subject,
            action,
            outcome: AuditOutcome::Denied,
            resource,
            reason: Some(reason.into()),
        }
    }
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    fn record(&self, _event: AuditEvent) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, event: AuditEvent) {
        let outcome = match event.outcome {
            AuditOutcome::Allowed => "allowed",
            AuditOutcome::Denied => "denied",
            AuditOutcome::Error => "error",
        };
        tracing::info!(
            request_id = %event.request_id,
            principal = ?event.subject.principal,
            token_id = ?event.subject.token_id,
            peer_addr = ?event.subject.peer_addr,
            action = ?event.action,
            resource = ?event.resource,
            outcome,
            reason = ?event.reason,
            "sync_audit_event"
        );
    }
}

/// JSON Lines audit sink for durable daemon authorization records.
pub struct JsonlAuditSink {
    file: Mutex<File>,
}

impl JsonlAuditSink {
    /// Open or create an append-only JSONL audit file.
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let mut options = OpenOptions::new();
        options.create(true).append(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let file = options.open(path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl AuditSink for JsonlAuditSink {
    fn record(&self, event: AuditEvent) {
        let Ok(mut file) = self.file.lock() else {
            tracing::error!("audit log lock poisoned");
            return;
        };

        if let Err(err) = serde_json::to_writer(&mut *file, &event) {
            tracing::error!(error = %err, "failed to write audit log event");
            return;
        }
        if let Err(err) = file
            .write_all(b"\n")
            .and_then(|_| file.flush())
            .and_then(|_| file.sync_all())
        {
            tracing::error!(error = %err, "failed to flush or sync audit log event");
        }
    }
}

/// Audit sink that forwards each event to two underlying sinks.
pub struct TeeAuditSink {
    left: Arc<dyn AuditSink>,
    right: Arc<dyn AuditSink>,
}

impl TeeAuditSink {
    /// Create a fan-out audit sink.
    pub fn new(left: Arc<dyn AuditSink>, right: Arc<dyn AuditSink>) -> Self {
        Self { left, right }
    }
}

impl AuditSink for TeeAuditSink {
    fn record(&self, event: AuditEvent) {
        self.left.record(event.clone());
        self.right.record(event);
    }
}

#[derive(Clone)]
pub struct ServiceSecurity {
    authorizer: Arc<dyn Authorizer>,
    audit_sink: Arc<dyn AuditSink>,
}

impl Default for ServiceSecurity {
    fn default() -> Self {
        Self {
            authorizer: Arc::new(AllowAllAuthorizer),
            audit_sink: Arc::new(TracingAuditSink),
        }
    }
}

impl ServiceSecurity {
    pub fn with_authorizer(mut self, authorizer: Arc<dyn Authorizer>) -> Self {
        self.authorizer = authorizer;
        self
    }

    pub fn with_audit_sink(mut self, audit_sink: Arc<dyn AuditSink>) -> Self {
        self.audit_sink = audit_sink;
        self
    }

    pub fn decision<T>(
        &self,
        request: &Request<T>,
        action: AuthorizationAction,
        resource: Option<String>,
    ) -> AuthorizationDecision {
        let auth_request = AuthorizationRequest {
            subject: subject_from_request(request),
            action,
            resource,
        };
        self.authorizer.authorize(&auth_request)
    }

    #[allow(clippy::result_large_err)]
    pub fn authorize<T>(
        &self,
        request: &Request<T>,
        action: AuthorizationAction,
        resource: Option<String>,
    ) -> Result<(), Status> {
        let subject = subject_from_request(request);
        let request_id = request_id_from_request(request);
        let auth_request = AuthorizationRequest {
            subject: subject.clone(),
            action: action.clone(),
            resource: resource.clone(),
        };

        match self.authorizer.authorize(&auth_request) {
            AuthorizationDecision::Allow => {
                self.audit_sink.record(AuditEvent::allowed(
                    now_ms(),
                    request_id,
                    subject,
                    action,
                    resource,
                ));
                Ok(())
            }
            AuthorizationDecision::Deny { reason } => {
                self.audit_sink.record(AuditEvent::denied(
                    now_ms(),
                    request_id,
                    subject,
                    action,
                    resource,
                    reason.clone(),
                ));
                Err(Status::permission_denied(reason))
            }
        }
    }

    pub fn allows<T>(
        &self,
        request: &Request<T>,
        action: AuthorizationAction,
        resource: Option<String>,
    ) -> bool {
        self.decision(request, action, resource).is_allowed()
    }
}

pub fn metadata_value<T>(request: &Request<T>, key: &str) -> Option<String> {
    request
        .metadata()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn request_id_from_request<T>(request: &Request<T>) -> String {
    metadata_value(request, REQUEST_ID_METADATA_KEY).unwrap_or_else(new_service_request_id)
}

pub fn subject_from_request<T>(request: &Request<T>) -> AuthorizationSubject {
    AuthorizationSubject {
        principal: metadata_value(request, PRINCIPAL_METADATA_KEY),
        token_id: metadata_value(request, TOKEN_ID_METADATA_KEY),
        peer_addr: request.remote_addr().map(|addr| addr.to_string()),
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn new_service_request_id() -> String {
    let seq = SERVICE_REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("service-{:x}-{seq:x}", now_ms())
}

#[derive(Debug, Clone, Copy)]
pub struct ReplayProtectionConfig {
    pub window: Duration,
    pub max_entries: usize,
}

impl Default for ReplayProtectionConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(5 * 60),
            max_entries: 10_000,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("replay nonce cannot be empty")]
    EmptyNonce,
    #[error("replayed request nonce")]
    Replay,
}

#[derive(Debug)]
pub struct ReplayProtector {
    config: ReplayProtectionConfig,
    seen: HashMap<String, Instant>,
}

impl ReplayProtector {
    pub fn new(config: ReplayProtectionConfig) -> Self {
        Self {
            config,
            seen: HashMap::new(),
        }
    }

    pub fn accept(&mut self, nonce: impl AsRef<str>, now: Instant) -> Result<(), ReplayError> {
        self.accept_scoped(nonce, "", now)
    }

    pub fn accept_scoped(
        &mut self,
        nonce: impl AsRef<str>,
        scope: impl AsRef<str>,
        now: Instant,
    ) -> Result<(), ReplayError> {
        let nonce = nonce.as_ref().trim();
        if nonce.is_empty() {
            return Err(ReplayError::EmptyNonce);
        }
        let key = format!("{}\n{nonce}", scope.as_ref());

        self.prune_expired(now);
        if self.seen.contains_key(&key) {
            return Err(ReplayError::Replay);
        }

        if self.seen.len() >= self.config.max_entries.max(1) {
            self.evict_oldest();
        }

        self.seen.insert(key, now);
        Ok(())
    }

    fn prune_expired(&mut self, now: Instant) {
        let window = self.config.window;
        self.seen
            .retain(|_, first_seen| now.duration_since(*first_seen) <= window);
    }

    fn evict_oldest(&mut self) {
        let Some(oldest) = self
            .seen
            .iter()
            .min_by_key(|(_, first_seen)| **first_seen)
            .map(|(nonce, _)| nonce.clone())
        else {
            return;
        };
        self.seen.remove(&oldest);
    }
}

#[derive(Debug)]
pub struct RateLimiter {
    capacity: u32,
    refill_per_second: f64,
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn per_minute(limit: u32, now: Instant) -> Self {
        let capacity = limit.max(1);
        Self {
            capacity,
            refill_per_second: capacity as f64 / 60.0,
            tokens: capacity as f64,
            last_refill: now,
        }
    }

    pub fn try_acquire(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.tokens < 1.0 {
            return false;
        }

        self.tokens -= 1.0;
        true
    }

    fn refill(&mut self, now: Instant) {
        if now <= self.last_refill {
            return;
        }

        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_second).min(self.capacity as f64);
        self.last_refill = now;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EvidenceFreshnessPolicy {
    pub require_revision_match: bool,
    pub require_evidence_after_revision: bool,
    pub require_expires_at: bool,
    pub require_runner_identity: bool,
    pub require_command: bool,
    pub require_exit_code: bool,
    pub require_log_or_artifact_digest: bool,
    pub max_age_ms: Option<u64>,
}

impl Default for EvidenceFreshnessPolicy {
    fn default() -> Self {
        Self {
            require_revision_match: true,
            require_evidence_after_revision: true,
            require_expires_at: true,
            require_runner_identity: true,
            require_command: true,
            require_exit_code: true,
            require_log_or_artifact_digest: true,
            max_age_ms: Some(24 * 60 * 60 * 1_000),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceFreshnessInput {
    pub revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_revision_id: Option<String>,
    pub revision_created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_created_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvidenceFreshnessError {
    #[error("evidence revision_id is missing")]
    MissingRevisionId,
    #[error("evidence revision_id does not match candidate revision")]
    RevisionMismatch,
    #[error("evidence timestamp is older than candidate revision")]
    EvidenceOlderThanRevision,
    #[error("evidence expires_at is missing")]
    MissingExpiresAt,
    #[error("evidence has expired")]
    Expired,
    #[error("evidence exceeds maximum freshness age")]
    MaxAgeExceeded,
    #[error("evidence runner identity is missing")]
    MissingRunnerIdentity,
    #[error("evidence command is missing")]
    MissingCommand,
    #[error("evidence exit code is missing")]
    MissingExitCode,
    #[error("evidence log or artifact digest is missing")]
    MissingDigest,
}

pub fn evaluate_evidence_freshness(
    policy: &EvidenceFreshnessPolicy,
    evidence: &EvidenceFreshnessInput,
    now_ms: u64,
) -> Result<(), EvidenceFreshnessError> {
    if policy.require_revision_match {
        let evidence_revision = evidence
            .evidence_revision_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(EvidenceFreshnessError::MissingRevisionId)?;
        if evidence_revision != evidence.revision_id {
            return Err(EvidenceFreshnessError::RevisionMismatch);
        }
    }

    if policy.require_evidence_after_revision {
        let evidence_created = evidence
            .evidence_created_at_ms
            .ok_or(EvidenceFreshnessError::EvidenceOlderThanRevision)?;
        if evidence_created < evidence.revision_created_at_ms {
            return Err(EvidenceFreshnessError::EvidenceOlderThanRevision);
        }
    }

    if policy.require_expires_at && evidence.evidence_expires_at_ms.is_none() {
        return Err(EvidenceFreshnessError::MissingExpiresAt);
    }

    if evidence
        .evidence_expires_at_ms
        .is_some_and(|expires_at| expires_at <= now_ms)
    {
        return Err(EvidenceFreshnessError::Expired);
    }

    if let (Some(max_age_ms), Some(evidence_created)) =
        (policy.max_age_ms, evidence.evidence_created_at_ms)
    {
        if now_ms.saturating_sub(evidence_created) > max_age_ms {
            return Err(EvidenceFreshnessError::MaxAgeExceeded);
        }
    }

    if policy.require_runner_identity
        && evidence
            .runner_identity
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(EvidenceFreshnessError::MissingRunnerIdentity);
    }

    if policy.require_command
        && evidence
            .command
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(EvidenceFreshnessError::MissingCommand);
    }

    if policy.require_exit_code && evidence.exit_code.is_none() {
        return Err(EvidenceFreshnessError::MissingExitCode);
    }

    if policy.require_log_or_artifact_digest
        && evidence
            .log_digest
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && evidence
            .artifact_digest
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(EvidenceFreshnessError::MissingDigest);
    }

    Ok(())
}

pub fn redact_authorization_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer"))
    {
        return "Bearer [REDACTED]".to_string();
    }

    "[REDACTED]".to_string()
}

pub fn redact_query_string(query: &str) -> String {
    query
        .split('&')
        .map(|part| {
            let Some((key, _value)) = part.split_once('=') else {
                return part.to_string();
            };

            if is_sensitive_key(key) {
                format!("{key}=[REDACTED]")
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub fn redacted_secret_marker(has_value: bool) -> Option<&'static str> {
    has_value.then_some("[REDACTED]")
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "authorization"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "token"
            | "bearer_token"
            | "api_key"
            | "secret"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_decision_reports_allowed_state() {
        assert!(AuthorizationDecision::Allow.is_allowed());
        assert!(!AuthorizationDecision::Deny {
            reason: "nope".to_string()
        }
        .is_allowed());
    }

    #[test]
    fn role_based_authorizer_allows_reader_read_actions_only() {
        let authorizer =
            RoleBasedAuthorizer::new().grant_role("agent-a", AuthorizationRole::Reader);
        let subject = AuthorizationSubject {
            principal: Some("agent-a".to_string()),
            token_id: None,
            peer_addr: None,
        };

        assert!(authorizer
            .authorize(&AuthorizationRequest {
                subject: subject.clone(),
                action: AuthorizationAction::FetchObjects,
                resource: None,
            })
            .is_allowed());
        assert!(authorizer
            .authorize(&AuthorizationRequest {
                subject: subject.clone(),
                action: AuthorizationAction::ReadIntent,
                resource: None,
            })
            .is_allowed());
        assert!(authorizer
            .authorize(&AuthorizationRequest {
                subject: subject.clone(),
                action: AuthorizationAction::VerifyCapsule,
                resource: None,
            })
            .is_allowed());
        assert!(!authorizer
            .authorize(&AuthorizationRequest {
                subject: subject.clone(),
                action: AuthorizationAction::UpdateRefs,
                resource: Some("heads/main".to_string()),
            })
            .is_allowed());
        assert!(!authorizer
            .authorize(&AuthorizationRequest {
                subject: subject.clone(),
                action: AuthorizationAction::CreateIntent,
                resource: None,
            })
            .is_allowed());
        assert!(!authorizer
            .authorize(&AuthorizationRequest {
                subject,
                action: AuthorizationAction::ReadPrivateCapsule,
                resource: None,
            })
            .is_allowed());
    }

    #[test]
    fn role_based_authorizer_supports_explicit_scopes() {
        let authorizer =
            RoleBasedAuthorizer::new().grant_scope("agent-a", AuthorizationScope::ObjectsWrite);
        let subject = AuthorizationSubject {
            principal: Some("agent-a".to_string()),
            token_id: None,
            peer_addr: None,
        };

        assert!(authorizer
            .authorize(&AuthorizationRequest {
                subject,
                action: AuthorizationAction::PushObjects,
                resource: None,
            })
            .is_allowed());
    }

    #[test]
    fn authorization_role_and_scope_parse_cli_values() {
        assert_eq!(
            "writer".parse::<AuthorizationRole>().unwrap(),
            AuthorizationRole::Writer
        );
        assert_eq!(
            "refs:write".parse::<AuthorizationScope>().unwrap(),
            AuthorizationScope::RefsWrite
        );
        assert_eq!(
            "capsules:private-read"
                .parse::<AuthorizationScope>()
                .unwrap(),
            AuthorizationScope::CapsulesPrivateRead
        );
        assert_eq!(
            "workstreams:*".parse::<AuthorizationScope>().unwrap(),
            AuthorizationScope::WorkstreamsAll
        );
        assert!("bogus".parse::<AuthorizationRole>().is_err());
    }

    #[test]
    fn audit_event_serializes_action_and_outcome() {
        let event = AuditEvent::denied(
            42,
            "req-1",
            AuthorizationSubject {
                principal: Some("agent-a".to_string()),
                token_id: Some("tok-1".to_string()),
                peer_addr: None,
            },
            AuthorizationAction::UpdateRefs,
            Some("heads/main".to_string()),
            "missing permission",
        );

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["action"], "update_refs");
        assert_eq!(value["outcome"], "denied");
        assert_eq!(value["subject"]["principal"], "agent-a");
        assert_eq!(value["reason"], "missing permission");
    }

    #[test]
    fn jsonl_audit_sink_appends_parseable_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::open(&path).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        sink.record(AuditEvent::allowed(
            42,
            "req-1",
            AuthorizationSubject {
                principal: Some("agent-a".to_string()),
                token_id: Some("tok-1".to_string()),
                peer_addr: Some("127.0.0.1:50051".to_string()),
            },
            AuthorizationAction::UpdateRefs,
            Some("heads/main".to_string()),
        ));
        sink.record(AuditEvent::denied(
            43,
            "req-2",
            AuthorizationSubject {
                principal: Some("agent-b".to_string()),
                token_id: None,
                peer_addr: None,
            },
            AuthorizationAction::ReadPrivateCapsule,
            Some("capsule-1".to_string()),
            "missing required scope capsules:private-read",
        ));

        let contents = std::fs::read_to_string(path).unwrap();
        let events: Vec<serde_json::Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["request_id"], "req-1");
        assert_eq!(events[0]["action"], "update_refs");
        assert_eq!(events[0]["outcome"], "allowed");
        assert_eq!(events[0]["resource"], "heads/main");
        assert_eq!(events[1]["action"], "read_private_capsule");
        assert_eq!(events[1]["outcome"], "denied");
        assert_eq!(
            events[1]["reason"],
            "missing required scope capsules:private-read"
        );
    }

    #[derive(Default)]
    struct RecordingAuditSink {
        events: Mutex<Vec<AuditEvent>>,
    }

    impl AuditSink for RecordingAuditSink {
        fn record(&self, event: AuditEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn service_security_allows_probe_does_not_emit_denied_audit_event() {
        let audit_sink = Arc::new(RecordingAuditSink::default());
        let security = ServiceSecurity::default()
            .with_authorizer(Arc::new(
                RoleBasedAuthorizer::new().grant_role("agent-a", AuthorizationRole::Reader),
            ))
            .with_audit_sink(audit_sink.clone());
        let mut request = Request::new(());
        request
            .metadata_mut()
            .insert(PRINCIPAL_METADATA_KEY, "agent-a".parse().unwrap());

        assert!(!security.allows(&request, AuthorizationAction::ReadPrivateCapsule, None));
        assert!(audit_sink.events.lock().unwrap().is_empty());

        security
            .authorize(&request, AuthorizationAction::ReadCapsule, None)
            .unwrap();
        let events = audit_sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, AuthorizationAction::ReadCapsule);
        assert_eq!(events[0].outcome, AuditOutcome::Allowed);
    }

    #[test]
    fn evidence_freshness_policy_rejects_expired_evidence() {
        let policy = EvidenceFreshnessPolicy::default();
        let evidence = EvidenceFreshnessInput {
            revision_id: "rev-a".to_string(),
            evidence_revision_id: Some("rev-a".to_string()),
            revision_created_at_ms: 1_000,
            evidence_created_at_ms: Some(1_100),
            evidence_expires_at_ms: Some(1_500),
            runner_identity: Some("ci-main".to_string()),
            command: Some("cargo test".to_string()),
            exit_code: Some(0),
            log_digest: Some("sha256:abc".to_string()),
            artifact_digest: None,
        };

        assert_eq!(
            evaluate_evidence_freshness(&policy, &evidence, 1_501),
            Err(EvidenceFreshnessError::Expired)
        );
    }

    #[test]
    fn evidence_freshness_policy_accepts_complete_fresh_evidence() {
        let policy = EvidenceFreshnessPolicy::default();
        let evidence = EvidenceFreshnessInput {
            revision_id: "rev-a".to_string(),
            evidence_revision_id: Some("rev-a".to_string()),
            revision_created_at_ms: 1_000,
            evidence_created_at_ms: Some(1_100),
            evidence_expires_at_ms: Some(10_000),
            runner_identity: Some("ci-main".to_string()),
            command: Some("cargo test".to_string()),
            exit_code: Some(0),
            log_digest: Some("sha256:abc".to_string()),
            artifact_digest: None,
        };

        assert!(evaluate_evidence_freshness(&policy, &evidence, 2_000).is_ok());
    }

    #[test]
    fn replay_protector_rejects_duplicate_nonce_inside_window() {
        let now = Instant::now();
        let mut protector = ReplayProtector::new(ReplayProtectionConfig {
            window: Duration::from_secs(60),
            max_entries: 16,
        });

        assert!(protector.accept("nonce-1", now).is_ok());
        assert_eq!(
            protector.accept("nonce-1", now + Duration::from_secs(1)),
            Err(ReplayError::Replay)
        );
    }

    #[test]
    fn replay_protector_allows_nonce_after_window() {
        let now = Instant::now();
        let mut protector = ReplayProtector::new(ReplayProtectionConfig {
            window: Duration::from_secs(1),
            max_entries: 16,
        });

        assert!(protector.accept("nonce-1", now).is_ok());
        assert!(protector
            .accept("nonce-1", now + Duration::from_secs(2))
            .is_ok());
    }

    #[test]
    fn replay_protector_binds_nonce_to_scope() {
        let now = Instant::now();
        let mut protector = ReplayProtector::new(ReplayProtectionConfig {
            window: Duration::from_secs(60),
            max_entries: 16,
        });

        assert!(protector
            .accept_scoped(
                "nonce-1",
                "principal=a;action=update_refs;resource=heads/main",
                now
            )
            .is_ok());
        assert!(protector
            .accept_scoped(
                "nonce-1",
                "principal=a;action=update_refs;resource=heads/feature",
                now + Duration::from_secs(1),
            )
            .is_ok());
        assert_eq!(
            protector.accept_scoped(
                "nonce-1",
                "principal=a;action=update_refs;resource=heads/main",
                now + Duration::from_secs(2),
            ),
            Err(ReplayError::Replay)
        );
    }

    #[test]
    fn rate_limiter_rejects_after_capacity_until_refill() {
        let now = Instant::now();
        let mut limiter = RateLimiter::per_minute(2, now);

        assert!(limiter.try_acquire(now));
        assert!(limiter.try_acquire(now));
        assert!(!limiter.try_acquire(now));
        assert!(limiter.try_acquire(now + Duration::from_secs(30)));
    }

    #[test]
    fn token_redaction_masks_authorization_and_query_tokens() {
        assert_eq!(
            redact_authorization_value("Bearer super-secret-token"),
            "Bearer [REDACTED]"
        );
        assert_eq!(
            redact_query_string("repo=demo&access_token=super-secret-token&token=other"),
            "repo=demo&access_token=[REDACTED]&token=[REDACTED]"
        );
    }
}
