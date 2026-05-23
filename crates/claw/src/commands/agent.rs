use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use claw_core::object::Object;
use claw_core::types::CapsulePublic;
use claw_crypto::keypair::KeyPair;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct AgentArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Register a new agent
    Register {
        /// Agent ID
        #[arg(short, long)]
        name: String,
        /// Agent version
        #[arg(short, long)]
        version: Option<String>,
        /// Register an externally managed Ed25519 public key instead of generating a local signing key
        #[arg(long)]
        public_key: Option<String>,
    },
    /// Generate a local agent signing key and print its public key
    Keygen {
        /// Agent ID used to choose the local key path
        #[arg(short, long)]
        name: String,
        /// Replace an existing local key for this agent name
        #[arg(long)]
        overwrite: bool,
    },
    /// Rotate an agent signing key and trust the replacement key
    Rotate {
        /// Agent ID
        #[arg(short, long)]
        name: String,
        /// Replacement agent version metadata
        #[arg(short, long)]
        version: Option<String>,
        /// Trust an externally managed replacement public key instead of generating a local signing key
        #[arg(long)]
        public_key: Option<String>,
        /// Validate and print the planned rotation without writing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Revoke an agent registration for future policy decisions
    Revoke {
        /// Agent ID
        #[arg(short, long)]
        name: String,
        /// Human-readable revocation reason
        #[arg(long)]
        reason: Option<String>,
        /// Validate and print the planned revocation without writing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Quarantine an agent registration without permanently revoking it
    Quarantine {
        /// Agent ID
        #[arg(short, long)]
        name: String,
        /// Human-readable quarantine reason
        #[arg(long)]
        reason: Option<String>,
        /// Validate and print the planned quarantine without writing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove an agent from quarantine
    Unquarantine {
        /// Agent ID
        #[arg(short, long)]
        name: String,
        /// Validate and print the planned unquarantine without writing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Apply a JSON fleet plan with many agent lifecycle operations
    Bulk {
        /// JSON file containing an `agents` array of lifecycle operations
        #[arg(long)]
        file: PathBuf,
        /// Validate and print the planned operations without writing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Audit registered agent keys and lifecycle states
    Audit {
        /// Only show agents with this lifecycle status: active|quarantined|revoked|legacy|malformed
        #[arg(long)]
        status: Option<String>,
        /// Only show agents with this risk level: none|info|warning|critical
        #[arg(long)]
        risk: Option<String>,
        /// Only show agents requiring operator action
        #[arg(long)]
        action_required: bool,
    },
    /// Show agent status
    Status {
        /// Agent name
        name: Option<String>,
    },
    /// List registered agents
    List,
}

const AGENT_SCHEMA_VERSION: u8 = 2;
const AGENT_AUDIT_JSON_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentRegistration {
    #[serde(default = "agent_schema_version")]
    pub schema_version: u8,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantined_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl AgentRegistration {
    pub(crate) fn is_revoked(&self) -> bool {
        self.revoked_at_ms.is_some()
    }

    pub(crate) fn is_quarantined(&self) -> bool {
        self.quarantined_at_ms.is_some()
    }

    pub(crate) fn lifecycle_status(&self) -> &'static str {
        if self.is_revoked() {
            "revoked"
        } else if self.is_quarantined() {
            "quarantined"
        } else {
            "active"
        }
    }

    fn public_key_prefix(&self) -> &str {
        let end = self.public_key.len().min(16);
        &self.public_key[..end]
    }
}

#[derive(Debug, Serialize)]
struct AgentAuditReport {
    schema_version: u8,
    action: &'static str,
    total_refs: usize,
    registered: usize,
    active: usize,
    revoked: usize,
    quarantined: usize,
    legacy: usize,
    malformed: usize,
    local_key_verified: usize,
    local_key_missing: usize,
    local_key_mismatch: usize,
    healthy: usize,
    action_required: usize,
    critical: usize,
    warning: usize,
    info: usize,
    filters: AgentAuditFilters,
    matching_agents: usize,
    agents: Vec<AgentAuditEntry>,
    findings: Vec<AgentAuditFinding>,
}

#[derive(Debug, Clone, Serialize)]
struct AgentAuditFilters {
    status: Option<String>,
    risk: Option<String>,
    action_required: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AgentAuditEntry {
    agent_id: String,
    ref_name: String,
    object_id: String,
    record_state: &'static str,
    status: &'static str,
    local_key_state: &'static str,
    risk_level: &'static str,
    action_required: bool,
    recommended_action: &'static str,
    schema_version: Option<u8>,
    agent_version: Option<String>,
    public_key: Option<String>,
    public_key_prefix: Option<String>,
    private_fields_present: bool,
    created_at_ms: Option<u64>,
    updated_at_ms: Option<u64>,
    revoked_at_ms: Option<u64>,
    revocation_reason: Option<String>,
    quarantined_at_ms: Option<u64>,
    quarantine_reason: Option<String>,
}

struct AgentAuditTriage {
    risk_level: &'static str,
    action_required: bool,
    recommended_action: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct AgentAuditFinding {
    agent_id: String,
    ref_name: String,
    severity: &'static str,
    code: &'static str,
    message: String,
}

#[derive(Debug, Deserialize)]
struct AgentBulkPlan {
    agents: Vec<AgentBulkAction>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum AgentBulkAction {
    Register {
        name: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        public_key: Option<String>,
    },
    Rotate {
        name: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        public_key: Option<String>,
    },
    Revoke {
        name: String,
        #[serde(default)]
        reason: Option<String>,
    },
    Quarantine {
        name: String,
        #[serde(default)]
        reason: Option<String>,
    },
    Unquarantine {
        name: String,
    },
}

#[derive(Debug, Serialize)]
struct AgentBulkReport {
    schema_version: u8,
    action: &'static str,
    dry_run: bool,
    planned_count: usize,
    changed_count: usize,
    results: Vec<AgentBulkResult>,
}

#[derive(Debug, Serialize)]
struct AgentBulkResult {
    index: usize,
    operation: &'static str,
    agent_id: String,
    dry_run: bool,
    changed: bool,
    current_status: Option<String>,
    status: Option<String>,
    public_key: Option<String>,
    object_id: Option<String>,
    reason: Option<String>,
    note: Option<String>,
}

fn empty_audit_entry(
    agent_id: String,
    ref_name: String,
    object_id: String,
    record_state: &'static str,
    status: &'static str,
    local_key_state: &'static str,
) -> AgentAuditEntry {
    let triage = audit_triage(record_state, status, local_key_state, false);
    AgentAuditEntry {
        agent_id,
        ref_name,
        object_id,
        record_state,
        status,
        local_key_state,
        risk_level: triage.risk_level,
        action_required: triage.action_required,
        recommended_action: triage.recommended_action,
        schema_version: None,
        agent_version: None,
        public_key: None,
        public_key_prefix: None,
        private_fields_present: false,
        created_at_ms: None,
        updated_at_ms: None,
        revoked_at_ms: None,
        revocation_reason: None,
        quarantined_at_ms: None,
        quarantine_reason: None,
    }
}

fn registered_audit_entry(
    record: &AgentRegistration,
    ref_name: String,
    object_id: String,
    record_state: &'static str,
    local_key_state: &'static str,
) -> AgentAuditEntry {
    let triage = audit_triage(
        record_state,
        record.lifecycle_status(),
        local_key_state,
        record.private_key.is_some(),
    );
    AgentAuditEntry {
        agent_id: record.agent_id.clone(),
        ref_name,
        object_id,
        record_state,
        status: record.lifecycle_status(),
        local_key_state,
        risk_level: triage.risk_level,
        action_required: triage.action_required,
        recommended_action: triage.recommended_action,
        schema_version: Some(record.schema_version),
        agent_version: record.agent_version.clone(),
        public_key: Some(record.public_key.clone()),
        public_key_prefix: Some(record.public_key_prefix().to_string()),
        private_fields_present: record.private_key.is_some(),
        created_at_ms: Some(record.created_at_ms),
        updated_at_ms: Some(record.updated_at_ms),
        revoked_at_ms: record.revoked_at_ms,
        revocation_reason: record.revocation_reason.clone(),
        quarantined_at_ms: record.quarantined_at_ms,
        quarantine_reason: record.quarantine_reason.clone(),
    }
}

fn public_key_prefix(public_key: &str) -> &str {
    let end = public_key.len().min(16);
    &public_key[..end]
}

fn audit_triage(
    record_state: &'static str,
    status: &'static str,
    local_key_state: &'static str,
    private_fields_present: bool,
) -> AgentAuditTriage {
    if matches!(record_state, "unreadable" | "non_blob" | "unrecognized")
        || (record_state == "malformed" && status != "legacy")
        || matches!(local_key_state, "mismatch" | "unreadable")
        || private_fields_present
    {
        return AgentAuditTriage {
            risk_level: "critical",
            action_required: true,
            recommended_action: "repair_or_reregister_agent_record",
        };
    }

    if record_state == "legacy" || status == "legacy" {
        return AgentAuditTriage {
            risk_level: "warning",
            action_required: true,
            recommended_action: "reregister_agent",
        };
    }

    if status == "quarantined" {
        return AgentAuditTriage {
            risk_level: "warning",
            action_required: true,
            recommended_action: "review_quarantine_then_rotate_or_unquarantine",
        };
    }

    if status == "revoked" {
        return AgentAuditTriage {
            risk_level: "info",
            action_required: false,
            recommended_action: "no_action_revoked_agent_retained_for_attribution",
        };
    }

    if local_key_state == "missing" {
        return AgentAuditTriage {
            risk_level: "info",
            action_required: false,
            recommended_action: "confirm_external_key_management_or_provision_local_key",
        };
    }

    AgentAuditTriage {
        risk_level: "none",
        action_required: false,
        recommended_action: "none",
    }
}

enum AgentRecordState {
    Missing,
    Registered(AgentRegistration),
    Legacy(CapsulePublic),
}

fn agent_schema_version() -> u8 {
    AGENT_SCHEMA_VERSION
}

fn now_ms() -> anyhow::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64)
}

fn claw_home_dir() -> anyhow::Result<std::path::PathBuf> {
    dirs::home_dir()
        .map(|dir| dir.join(".claw"))
        .ok_or_else(|| anyhow::anyhow!("could not find home directory"))
}

fn agent_keys_dir() -> anyhow::Result<std::path::PathBuf> {
    Ok(claw_home_dir()?.join("agent-keys"))
}

fn agent_key_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let digest = hex::encode(hasher.finalize());
    Ok(agent_keys_dir()?.join(format!("{digest}.ed25519")))
}

fn set_private_permissions(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(not(unix))]
    let _ = path;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn save_local_agent_key(name: &str, keypair: &KeyPair) -> anyhow::Result<()> {
    let path = agent_key_path(name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    keypair.save_to_file(&path)?;
    set_private_permissions(&path)?;
    Ok(())
}

fn load_local_agent_key(name: &str) -> anyhow::Result<Option<KeyPair>> {
    let path = agent_key_path(name)?;
    if !path.exists() {
        return Ok(None);
    }
    let keypair =
        KeyPair::load_from_file(&path).map_err(|e| anyhow::anyhow!("invalid local key: {e}"))?;
    set_private_permissions(&path)?;
    Ok(Some(keypair))
}

fn decode_hex_32(value: &str, field: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = hex::decode(value).map_err(|e| anyhow::anyhow!("invalid {field}: {e}"))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {field}: expected 32 bytes"))
}

fn normalize_public_key(public_key: &str) -> anyhow::Result<String> {
    let bytes = decode_hex_32(public_key, "public key")?;
    Ok(hex::encode(bytes))
}

fn keypair_from_private(private_hex: &str, expected_public: &[u8; 32]) -> anyhow::Result<KeyPair> {
    let private = decode_hex_32(private_hex, "private key")?;
    let keypair = KeyPair::from_bytes(&private)?;
    if keypair.public_key_bytes() != *expected_public {
        anyhow::bail!("agent key mismatch: stored public key does not match private key");
    }
    Ok(keypair)
}

fn ensure_local_key_for_registration(
    record: &AgentRegistration,
    name: &str,
) -> anyhow::Result<bool> {
    let expected_public = decode_hex_32(&record.public_key, "public key")?;

    if let Some(local_key) = load_local_agent_key(name)? {
        if local_key.public_key_bytes() == expected_public {
            return Ok(true);
        }

        if let Some(private_hex) = record.private_key.as_deref() {
            let recovered = keypair_from_private(private_hex, &expected_public)?;
            save_local_agent_key(name, &recovered)?;
            return Ok(true);
        }

        anyhow::bail!("agent key mismatch: local key does not match stored public key");
    }

    if let Some(private_hex) = record.private_key.as_deref() {
        let recovered = keypair_from_private(private_hex, &expected_public)?;
        save_local_agent_key(name, &recovered)?;
        return Ok(true);
    }

    Ok(false)
}

fn registration_keypair(record: &AgentRegistration, name: &str) -> anyhow::Result<KeyPair> {
    let expected_public = decode_hex_32(&record.public_key, "public key")?;
    let Some(local_key) = load_local_agent_key(name)? else {
        anyhow::bail!("local signing key not found for agent '{name}'");
    };
    if local_key.public_key_bytes() != expected_public {
        anyhow::bail!("agent key mismatch: local key does not match stored public key");
    }
    Ok(local_key)
}

fn read_agent_record(store: &ClawStore, name: &str) -> anyhow::Result<AgentRecordState> {
    let Some(id) = store.get_ref(&format!("agents/{name}"))? else {
        return Ok(AgentRecordState::Missing);
    };
    let obj = store.load_object(&id)?;
    let Object::Blob(blob) = obj else {
        anyhow::bail!("agent ref points to non-blob object");
    };

    if let Ok(record) = serde_json::from_slice::<AgentRegistration>(&blob.data) {
        return Ok(AgentRecordState::Registered(record));
    }
    if let Ok(legacy) = serde_json::from_slice::<CapsulePublic>(&blob.data) {
        return Ok(AgentRecordState::Legacy(legacy));
    }

    anyhow::bail!("agent record format is not recognized")
}

fn store_agent_registration(
    store: &ClawStore,
    name: &str,
    record: &AgentRegistration,
) -> anyhow::Result<claw_core::id::ObjectId> {
    let mut sanitized = record.clone();
    sanitized.private_key = None;
    let serialized =
        serde_json::to_vec(&sanitized).map_err(|e| anyhow::anyhow!("serialization failed: {e}"))?;
    let blob = Object::Blob(claw_core::types::Blob {
        data: serialized,
        media_type: Some("application/json".to_string()),
    });
    let id = store.store_object(&blob)?;
    store.set_ref(&format!("agents/{name}"), &id)?;
    Ok(id)
}

fn new_registration(
    name: &str,
    version: Option<String>,
) -> anyhow::Result<(AgentRegistration, KeyPair)> {
    let now = now_ms()?;
    let keypair = KeyPair::generate();
    Ok((
        AgentRegistration {
            schema_version: AGENT_SCHEMA_VERSION,
            agent_id: name.to_string(),
            agent_version: version,
            public_key: hex::encode(keypair.public_key_bytes()),
            private_key: None,
            revoked_at_ms: None,
            revocation_reason: None,
            quarantined_at_ms: None,
            quarantine_reason: None,
            created_at_ms: now,
            updated_at_ms: now,
        },
        keypair,
    ))
}

fn new_external_registration(
    name: &str,
    version: Option<String>,
    public_key: String,
) -> anyhow::Result<AgentRegistration> {
    let now = now_ms()?;
    Ok(AgentRegistration {
        schema_version: AGENT_SCHEMA_VERSION,
        agent_id: name.to_string(),
        agent_version: version,
        public_key: normalize_public_key(&public_key)?,
        private_key: None,
        revoked_at_ms: None,
        revocation_reason: None,
        quarantined_at_ms: None,
        quarantine_reason: None,
        created_at_ms: now,
        updated_at_ms: now,
    })
}

pub(crate) fn ensure_registered_signing_agent(
    store: &ClawStore,
    name: &str,
) -> anyhow::Result<AgentRegistration> {
    match read_agent_record(store, name)? {
        AgentRecordState::Registered(mut record) => {
            if record.is_revoked() {
                anyhow::bail!(
                    "agent '{name}' is revoked; run `claw agent rotate --name {name}` to trust a replacement key"
                );
            }
            if record.is_quarantined() {
                anyhow::bail!(
                    "agent '{name}' is quarantined; run `claw agent unquarantine --name {name}` or rotate the key before signing"
                );
            }
            if !ensure_local_key_for_registration(&record, name)? {
                // Existing metadata without local key: rotate to a new local-only private key.
                let (mut rotated, keypair) = new_registration(name, record.agent_version.clone())?;
                rotated.created_at_ms = record.created_at_ms;
                rotated.updated_at_ms = now_ms()?;
                save_local_agent_key(name, &keypair)?;
                store_agent_registration(store, name, &rotated)?;
                return Ok(rotated);
            }

            if record.private_key.is_some() {
                record.private_key = None;
                record.updated_at_ms = now_ms()?;
                store_agent_registration(store, name, &record)?;
            }
            Ok(record)
        }
        AgentRecordState::Legacy(legacy) => {
            let (record, keypair) = new_registration(name, legacy.agent_version)?;
            save_local_agent_key(name, &keypair)?;
            store_agent_registration(store, name, &record)?;
            Ok(record)
        }
        AgentRecordState::Missing => {
            let (record, keypair) = new_registration(name, None)?;
            save_local_agent_key(name, &keypair)?;
            store_agent_registration(store, name, &record)?;
            Ok(record)
        }
    }
}

pub(crate) fn keypair_for_agent(
    agent_name: &str,
    record: &AgentRegistration,
) -> anyhow::Result<KeyPair> {
    registration_keypair(record, agent_name)
}

fn audit_agent_records(store: &ClawStore) -> anyhow::Result<AgentAuditReport> {
    let refs = store.list_refs("agents")?;
    let mut report = AgentAuditReport {
        schema_version: AGENT_AUDIT_JSON_SCHEMA_VERSION,
        action: "agent.audit",
        total_refs: refs.len(),
        registered: 0,
        active: 0,
        revoked: 0,
        quarantined: 0,
        legacy: 0,
        malformed: 0,
        local_key_verified: 0,
        local_key_missing: 0,
        local_key_mismatch: 0,
        healthy: 0,
        action_required: 0,
        critical: 0,
        warning: 0,
        info: 0,
        filters: AgentAuditFilters {
            status: None,
            risk: None,
            action_required: false,
        },
        matching_agents: 0,
        agents: Vec::new(),
        findings: Vec::new(),
    };

    for (ref_name, id) in refs {
        let agent_name = ref_name.trim_start_matches("agents/").to_string();
        let object = match store.load_object(&id) {
            Ok(object) => object,
            Err(err) => {
                report.malformed += 1;
                report.findings.push(AgentAuditFinding {
                    agent_id: agent_name.clone(),
                    ref_name: ref_name.clone(),
                    severity: "error",
                    code: "object_load_failed",
                    message: format!("agent ref points to an unreadable object: {err}"),
                });
                push_audit_entry(
                    &mut report,
                    empty_audit_entry(
                        agent_name,
                        ref_name,
                        id.to_string(),
                        "unreadable",
                        "unreadable",
                        "not_checked",
                    ),
                );
                continue;
            }
        };

        let Object::Blob(blob) = object else {
            report.malformed += 1;
            report.findings.push(AgentAuditFinding {
                agent_id: agent_name.clone(),
                ref_name: ref_name.clone(),
                severity: "error",
                code: "non_blob_agent_record",
                message: "agent ref must point to a JSON blob".to_string(),
            });
            push_audit_entry(
                &mut report,
                empty_audit_entry(
                    agent_name,
                    ref_name,
                    id.to_string(),
                    "non_blob",
                    "malformed",
                    "not_checked",
                ),
            );
            continue;
        };

        if let Ok(record) = serde_json::from_slice::<AgentRegistration>(&blob.data) {
            let mut record_state = "registered";
            let mut local_key_state = "not_checked";
            report.registered += 1;
            match record.lifecycle_status() {
                "revoked" => report.revoked += 1,
                "quarantined" => report.quarantined += 1,
                _ => report.active += 1,
            }

            if record.private_key.is_some() {
                report.findings.push(AgentAuditFinding {
                    agent_id: record.agent_id.clone(),
                    ref_name: ref_name.clone(),
                    severity: "error",
                    code: "private_key_material_in_record",
                    message: "repository agent metadata contains private key material".to_string(),
                });
            }

            let expected_public = match decode_hex_32(&record.public_key, "public key") {
                Ok(public_key) => public_key,
                Err(err) => {
                    record_state = "malformed";
                    report.malformed += 1;
                    report.findings.push(AgentAuditFinding {
                        agent_id: record.agent_id.clone(),
                        ref_name: ref_name.clone(),
                        severity: "error",
                        code: "invalid_public_key",
                        message: err.to_string(),
                    });
                    push_audit_entry(
                        &mut report,
                        registered_audit_entry(
                            &record,
                            ref_name,
                            id.to_string(),
                            record_state,
                            local_key_state,
                        ),
                    );
                    continue;
                }
            };

            match load_local_agent_key(&agent_name) {
                Ok(Some(local_key)) => {
                    if local_key.public_key_bytes() == expected_public {
                        local_key_state = "verified";
                        report.local_key_verified += 1;
                    } else {
                        local_key_state = "mismatch";
                        report.local_key_mismatch += 1;
                        report.findings.push(AgentAuditFinding {
                            agent_id: record.agent_id.clone(),
                            ref_name: ref_name.clone(),
                            severity: "error",
                            code: "local_key_mismatch",
                            message: "local signing key does not match the trusted public key"
                                .to_string(),
                        });
                    }
                }
                Ok(None) => {
                    local_key_state = "missing";
                    report.local_key_missing += 1;
                    if !record.is_revoked() {
                        report.findings.push(AgentAuditFinding {
                            agent_id: record.agent_id.clone(),
                            ref_name: ref_name.clone(),
                            severity: "info",
                            code: "local_key_absent",
                            message: "no local signing key is present on this machine; this is expected for externally managed public-key registrations".to_string(),
                        });
                    }
                }
                Err(err) => {
                    local_key_state = "unreadable";
                    report.local_key_mismatch += 1;
                    report.findings.push(AgentAuditFinding {
                        agent_id: record.agent_id.clone(),
                        ref_name: ref_name.clone(),
                        severity: "error",
                        code: "local_key_unreadable",
                        message: format!("local signing key could not be read: {err}"),
                    });
                }
            }

            if record.is_quarantined() {
                report.findings.push(AgentAuditFinding {
                    agent_id: record.agent_id.clone(),
                    ref_name: ref_name.clone(),
                    severity: "warning",
                    code: "agent_quarantined",
                    message: record
                        .quarantine_reason
                        .clone()
                        .unwrap_or_else(|| "agent is quarantined".to_string()),
                });
            }

            push_audit_entry(
                &mut report,
                registered_audit_entry(
                    &record,
                    ref_name,
                    id.to_string(),
                    record_state,
                    local_key_state,
                ),
            );
            continue;
        }

        if let Ok(legacy) = serde_json::from_slice::<CapsulePublic>(&blob.data) {
            report.legacy += 1;
            report.findings.push(AgentAuditFinding {
                agent_id: legacy.agent_id.clone(),
                ref_name: ref_name.clone(),
                severity: "warning",
                code: "legacy_agent_record",
                message: "legacy agent registration should be re-registered before fleet rotation or quarantine".to_string(),
            });
            let mut entry = empty_audit_entry(
                legacy.agent_id,
                ref_name,
                id.to_string(),
                "legacy",
                "legacy",
                "not_applicable",
            );
            entry.agent_version = legacy.agent_version;
            push_audit_entry(&mut report, entry);
            continue;
        }

        report.malformed += 1;
        report.findings.push(AgentAuditFinding {
            agent_id: agent_name.clone(),
            ref_name: ref_name.clone(),
            severity: "error",
            code: "unrecognized_agent_record",
            message: "agent record is neither a registration nor a legacy capsule public record"
                .to_string(),
        });
        push_audit_entry(
            &mut report,
            empty_audit_entry(
                agent_name,
                ref_name,
                id.to_string(),
                "unrecognized",
                "malformed",
                "not_checked",
            ),
        );
    }

    report.matching_agents = report.agents.len();
    Ok(report)
}

fn filter_agent_audit_report(
    mut report: AgentAuditReport,
    filters: AgentAuditFilters,
) -> anyhow::Result<AgentAuditReport> {
    if let Some(status) = filters.status.as_deref() {
        validate_audit_status(status)?;
    }
    if let Some(risk) = filters.risk.as_deref() {
        validate_audit_risk(risk)?;
    }

    report.agents.retain(|agent| {
        filters
            .status
            .as_deref()
            .is_none_or(|status| agent.status.eq_ignore_ascii_case(status))
            && filters
                .risk
                .as_deref()
                .is_none_or(|risk| agent.risk_level.eq_ignore_ascii_case(risk))
            && (!filters.action_required || agent.action_required)
    });
    let matching_refs = report
        .agents
        .iter()
        .map(|agent| agent.ref_name.clone())
        .collect::<std::collections::HashSet<_>>();
    report
        .findings
        .retain(|finding| matching_refs.contains(&finding.ref_name));
    report.matching_agents = report.agents.len();
    report.filters = filters;
    Ok(report)
}

fn validate_audit_status(status: &str) -> anyhow::Result<()> {
    if matches!(
        status.to_ascii_lowercase().as_str(),
        "active" | "quarantined" | "revoked" | "legacy" | "malformed" | "unreadable"
    ) {
        Ok(())
    } else {
        anyhow::bail!(
            "invalid audit status '{status}'; expected active|quarantined|revoked|legacy|malformed"
        )
    }
}

fn validate_audit_risk(risk: &str) -> anyhow::Result<()> {
    if matches!(
        risk.to_ascii_lowercase().as_str(),
        "none" | "info" | "warning" | "critical"
    ) {
        Ok(())
    } else {
        anyhow::bail!("invalid audit risk '{risk}'; expected none|info|warning|critical")
    }
}

fn load_bulk_plan(path: &Path) -> anyhow::Result<AgentBulkPlan> {
    let bytes = std::fs::read(path).map_err(|err| {
        anyhow::anyhow!("failed to read agent bulk plan {}: {err}", path.display())
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|err| anyhow::anyhow!("agent bulk plan must be JSON: {err}"))
}

fn run_bulk_plan(
    store: &ClawStore,
    plan: AgentBulkPlan,
    dry_run: bool,
) -> anyhow::Result<AgentBulkReport> {
    let mut results = Vec::new();
    if dry_run {
        let mut simulated_status = HashMap::new();
        for (index, action) in plan.agents.into_iter().enumerate() {
            results.push(plan_bulk_action_dry_run(
                store,
                &mut simulated_status,
                index,
                action,
            )?);
        }
    } else {
        for (index, action) in plan.agents.into_iter().enumerate() {
            results.push(apply_bulk_action(store, index, action, dry_run)?);
        }
    }
    let changed_count = results.iter().filter(|result| result.changed).count();
    Ok(AgentBulkReport {
        schema_version: 1,
        action: "agent.bulk",
        dry_run,
        planned_count: results.len(),
        changed_count,
        results,
    })
}

fn plan_bulk_action_dry_run(
    store: &ClawStore,
    simulated_status: &mut HashMap<String, String>,
    index: usize,
    action: AgentBulkAction,
) -> anyhow::Result<AgentBulkResult> {
    let name = action_agent_name(&action).to_string();
    let current_status = simulated_status
        .get(&name)
        .cloned()
        .or_else(|| record_status(&read_agent_record(store, &name).ok()?));

    match action {
        AgentBulkAction::Register {
            name, public_key, ..
        } => {
            if current_status.as_deref() == Some("revoked") {
                anyhow::bail!(
                    "bulk operation {index} cannot register revoked agent '{name}'; use rotate"
                );
            }
            let normalized_public_key = public_key
                .as_deref()
                .map(normalize_public_key)
                .transpose()?;
            simulated_status.insert(name.clone(), "active".to_string());
            Ok(AgentBulkResult {
                index,
                operation: "register",
                agent_id: name,
                dry_run: true,
                changed: true,
                current_status,
                status: Some("active".to_string()),
                public_key: normalized_public_key,
                object_id: None,
                reason: None,
                note: Some(
                    "validated registration plan without writing repository refs or local keys"
                        .to_string(),
                ),
            })
        }
        AgentBulkAction::Rotate {
            name, public_key, ..
        } => {
            require_planned_agent(index, "rotate", &name, current_status.as_deref())?;
            let normalized_public_key = public_key
                .as_deref()
                .map(normalize_public_key)
                .transpose()?;
            simulated_status.insert(name.clone(), "active".to_string());
            Ok(AgentBulkResult {
                index,
                operation: "rotate",
                agent_id: name,
                dry_run: true,
                changed: true,
                current_status,
                status: Some("active".to_string()),
                public_key: normalized_public_key,
                object_id: None,
                reason: None,
                note: Some(
                    "validated rotation plan without writing repository refs or local keys"
                        .to_string(),
                ),
            })
        }
        AgentBulkAction::Revoke { name, reason } => {
            require_planned_agent(index, "revoke", &name, current_status.as_deref())?;
            simulated_status.insert(name.clone(), "revoked".to_string());
            Ok(AgentBulkResult {
                index,
                operation: "revoke",
                agent_id: name,
                dry_run: true,
                changed: current_status.as_deref() != Some("revoked"),
                current_status,
                status: Some("revoked".to_string()),
                public_key: None,
                object_id: None,
                reason,
                note: Some("validated revocation plan without writing repository refs".to_string()),
            })
        }
        AgentBulkAction::Quarantine { name, reason } => {
            require_planned_agent(index, "quarantine", &name, current_status.as_deref())?;
            if current_status.as_deref() == Some("revoked") {
                anyhow::bail!("bulk operation {index} cannot quarantine revoked agent '{name}'");
            }
            simulated_status.insert(name.clone(), "quarantined".to_string());
            Ok(AgentBulkResult {
                index,
                operation: "quarantine",
                agent_id: name,
                dry_run: true,
                changed: current_status.as_deref() != Some("quarantined"),
                current_status,
                status: Some("quarantined".to_string()),
                public_key: None,
                object_id: None,
                reason,
                note: Some("validated quarantine plan without writing repository refs".to_string()),
            })
        }
        AgentBulkAction::Unquarantine { name } => {
            require_planned_agent(index, "unquarantine", &name, current_status.as_deref())?;
            if current_status.as_deref() == Some("revoked") {
                anyhow::bail!("bulk operation {index} cannot unquarantine revoked agent '{name}'");
            }
            simulated_status.insert(name.clone(), "active".to_string());
            Ok(AgentBulkResult {
                index,
                operation: "unquarantine",
                agent_id: name,
                dry_run: true,
                changed: current_status.as_deref() == Some("quarantined"),
                current_status,
                status: Some("active".to_string()),
                public_key: None,
                object_id: None,
                reason: None,
                note: Some(
                    "validated unquarantine plan without writing repository refs".to_string(),
                ),
            })
        }
    }
}

fn action_agent_name(action: &AgentBulkAction) -> &str {
    match action {
        AgentBulkAction::Register { name, .. }
        | AgentBulkAction::Rotate { name, .. }
        | AgentBulkAction::Revoke { name, .. }
        | AgentBulkAction::Quarantine { name, .. }
        | AgentBulkAction::Unquarantine { name } => name,
    }
}

fn require_planned_agent(
    index: usize,
    operation: &str,
    name: &str,
    status: Option<&str>,
) -> anyhow::Result<()> {
    match status {
        Some("legacy") => anyhow::bail!(
            "bulk operation {index} cannot {operation} legacy agent '{name}'; register it first"
        ),
        Some(_) => Ok(()),
        None => anyhow::bail!("bulk operation {index} cannot {operation} missing agent '{name}'"),
    }
}

fn apply_bulk_action(
    store: &ClawStore,
    index: usize,
    action: AgentBulkAction,
    dry_run: bool,
) -> anyhow::Result<AgentBulkResult> {
    match action {
        AgentBulkAction::Register {
            name,
            version,
            public_key,
        } => bulk_register(store, index, name, version, public_key, dry_run),
        AgentBulkAction::Rotate {
            name,
            version,
            public_key,
        } => bulk_rotate(store, index, name, version, public_key, dry_run),
        AgentBulkAction::Revoke { name, reason } => {
            bulk_revoke(store, index, name, reason, dry_run)
        }
        AgentBulkAction::Quarantine { name, reason } => {
            bulk_quarantine(store, index, name, reason, dry_run)
        }
        AgentBulkAction::Unquarantine { name } => bulk_unquarantine(store, index, name, dry_run),
    }
}

fn bulk_register(
    store: &ClawStore,
    index: usize,
    name: String,
    version: Option<String>,
    public_key: Option<String>,
    dry_run: bool,
) -> anyhow::Result<AgentBulkResult> {
    let current = read_agent_record(store, &name)?;
    let current_status = record_status(&current);
    if matches!(
        current,
        AgentRecordState::Registered(ref record) if record.is_revoked()
    ) {
        anyhow::bail!("bulk operation {index} cannot register revoked agent '{name}'; use rotate");
    }
    let normalized_public_key = public_key
        .as_deref()
        .map(normalize_public_key)
        .transpose()?;
    if dry_run {
        return Ok(AgentBulkResult {
            index,
            operation: "register",
            agent_id: name,
            dry_run,
            changed: true,
            current_status,
            status: Some("active".to_string()),
            public_key: normalized_public_key,
            object_id: None,
            reason: None,
            note: Some(
                "validated registration plan without writing repository refs or local keys"
                    .to_string(),
            ),
        });
    }

    let requested_version = version.clone();
    let record = match current {
        AgentRecordState::Registered(mut existing) => {
            if let Some(v) = requested_version {
                existing.agent_version = Some(v);
            }
            existing.private_key = None;
            if let Some(public_key) = normalized_public_key {
                existing.public_key = public_key;
            } else {
                match ensure_local_key_for_registration(&existing, &name) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        let keypair = KeyPair::generate();
                        existing.public_key = hex::encode(keypair.public_key_bytes());
                        save_local_agent_key(&name, &keypair)?;
                    }
                }
            }
            existing.updated_at_ms = now_ms()?;
            existing
        }
        AgentRecordState::Legacy(legacy) => {
            let version = requested_version.or(legacy.agent_version);
            if let Some(public_key) = normalized_public_key {
                new_external_registration(&name, version, public_key)?
            } else {
                let (record, keypair) = new_registration(&name, version)?;
                save_local_agent_key(&name, &keypair)?;
                record
            }
        }
        AgentRecordState::Missing => {
            if let Some(public_key) = normalized_public_key {
                new_external_registration(&name, requested_version, public_key)?
            } else {
                let (record, keypair) = new_registration(&name, requested_version)?;
                save_local_agent_key(&name, &keypair)?;
                record
            }
        }
    };
    let public_key = record.public_key.clone();
    let status = record.lifecycle_status().to_string();
    let id = store_agent_registration(store, &name, &record)?;
    Ok(AgentBulkResult {
        index,
        operation: "register",
        agent_id: name,
        dry_run,
        changed: true,
        current_status,
        status: Some(status),
        public_key: Some(public_key),
        object_id: Some(id.to_string()),
        reason: None,
        note: None,
    })
}

fn bulk_rotate(
    store: &ClawStore,
    index: usize,
    name: String,
    version: Option<String>,
    public_key: Option<String>,
    dry_run: bool,
) -> anyhow::Result<AgentBulkResult> {
    let current = registered_record_for_bulk(store, index, &name, "rotate")?;
    let current_status = Some(current.lifecycle_status().to_string());
    let replacement_version = version.or(current.agent_version.clone());
    let normalized_public_key = public_key
        .as_deref()
        .map(normalize_public_key)
        .transpose()?;
    if dry_run {
        return Ok(AgentBulkResult {
            index,
            operation: "rotate",
            agent_id: name,
            dry_run,
            changed: true,
            current_status,
            status: Some("active".to_string()),
            public_key: normalized_public_key,
            object_id: None,
            reason: None,
            note: Some(
                "validated rotation plan without writing repository refs or local keys".to_string(),
            ),
        });
    }

    let mut rotated = if let Some(public_key) = normalized_public_key {
        new_external_registration(&name, replacement_version, public_key)?
    } else {
        let (record, keypair) = new_registration(&name, replacement_version)?;
        save_local_agent_key(&name, &keypair)?;
        record
    };
    rotated.created_at_ms = current.created_at_ms;
    rotated.updated_at_ms = now_ms()?;
    let public_key = rotated.public_key.clone();
    let id = store_agent_registration(store, &name, &rotated)?;
    Ok(AgentBulkResult {
        index,
        operation: "rotate",
        agent_id: name,
        dry_run,
        changed: true,
        current_status,
        status: Some("active".to_string()),
        public_key: Some(public_key),
        object_id: Some(id.to_string()),
        reason: None,
        note: None,
    })
}

fn bulk_revoke(
    store: &ClawStore,
    index: usize,
    name: String,
    reason: Option<String>,
    dry_run: bool,
) -> anyhow::Result<AgentBulkResult> {
    let mut record = registered_record_for_bulk(store, index, &name, "revoke")?;
    let current_status = Some(record.lifecycle_status().to_string());
    if dry_run || record.is_revoked() {
        return Ok(AgentBulkResult {
            index,
            operation: "revoke",
            agent_id: name,
            dry_run,
            changed: !record.is_revoked(),
            current_status,
            status: Some("revoked".to_string()),
            public_key: Some(record.public_key),
            object_id: None,
            reason,
            note: if dry_run {
                Some("validated revocation plan without writing repository refs".to_string())
            } else {
                Some("agent was already revoked".to_string())
            },
        });
    }

    record.revoked_at_ms = Some(now_ms()?);
    record.revocation_reason = reason.clone();
    record.updated_at_ms = now_ms()?;
    let public_key = record.public_key.clone();
    let id = store_agent_registration(store, &name, &record)?;
    Ok(AgentBulkResult {
        index,
        operation: "revoke",
        agent_id: name,
        dry_run,
        changed: true,
        current_status,
        status: Some("revoked".to_string()),
        public_key: Some(public_key),
        object_id: Some(id.to_string()),
        reason,
        note: None,
    })
}

fn bulk_quarantine(
    store: &ClawStore,
    index: usize,
    name: String,
    reason: Option<String>,
    dry_run: bool,
) -> anyhow::Result<AgentBulkResult> {
    let mut record = registered_record_for_bulk(store, index, &name, "quarantine")?;
    let current_status = Some(record.lifecycle_status().to_string());
    if record.is_revoked() {
        anyhow::bail!("bulk operation {index} cannot quarantine revoked agent '{name}'");
    }
    if dry_run || record.is_quarantined() {
        return Ok(AgentBulkResult {
            index,
            operation: "quarantine",
            agent_id: name,
            dry_run,
            changed: !record.is_quarantined(),
            current_status,
            status: Some("quarantined".to_string()),
            public_key: Some(record.public_key),
            object_id: None,
            reason,
            note: if dry_run {
                Some("validated quarantine plan without writing repository refs".to_string())
            } else {
                Some("agent was already quarantined".to_string())
            },
        });
    }

    record.quarantined_at_ms = Some(now_ms()?);
    record.quarantine_reason = reason.clone();
    record.updated_at_ms = now_ms()?;
    let public_key = record.public_key.clone();
    let id = store_agent_registration(store, &name, &record)?;
    Ok(AgentBulkResult {
        index,
        operation: "quarantine",
        agent_id: name,
        dry_run,
        changed: true,
        current_status,
        status: Some("quarantined".to_string()),
        public_key: Some(public_key),
        object_id: Some(id.to_string()),
        reason,
        note: None,
    })
}

fn bulk_unquarantine(
    store: &ClawStore,
    index: usize,
    name: String,
    dry_run: bool,
) -> anyhow::Result<AgentBulkResult> {
    let mut record = registered_record_for_bulk(store, index, &name, "unquarantine")?;
    let current_status = Some(record.lifecycle_status().to_string());
    if record.is_revoked() {
        anyhow::bail!("bulk operation {index} cannot unquarantine revoked agent '{name}'");
    }
    if dry_run || !record.is_quarantined() {
        return Ok(AgentBulkResult {
            index,
            operation: "unquarantine",
            agent_id: name,
            dry_run,
            changed: record.is_quarantined(),
            current_status,
            status: Some("active".to_string()),
            public_key: Some(record.public_key),
            object_id: None,
            reason: None,
            note: if dry_run {
                Some("validated unquarantine plan without writing repository refs".to_string())
            } else {
                Some("agent was not quarantined".to_string())
            },
        });
    }

    record.quarantined_at_ms = None;
    record.quarantine_reason = None;
    record.updated_at_ms = now_ms()?;
    let public_key = record.public_key.clone();
    let id = store_agent_registration(store, &name, &record)?;
    Ok(AgentBulkResult {
        index,
        operation: "unquarantine",
        agent_id: name,
        dry_run,
        changed: true,
        current_status,
        status: Some("active".to_string()),
        public_key: Some(public_key),
        object_id: Some(id.to_string()),
        reason: None,
        note: None,
    })
}

fn registered_record_for_bulk(
    store: &ClawStore,
    index: usize,
    name: &str,
    operation: &str,
) -> anyhow::Result<AgentRegistration> {
    match read_agent_record(store, name)? {
        AgentRecordState::Registered(record) => Ok(record),
        AgentRecordState::Legacy(_) => anyhow::bail!(
            "bulk operation {index} cannot {operation} legacy agent '{name}'; register it first"
        ),
        AgentRecordState::Missing => {
            anyhow::bail!("bulk operation {index} cannot {operation} missing agent '{name}'")
        }
    }
}

fn record_status(record: &AgentRecordState) -> Option<String> {
    match record {
        AgentRecordState::Missing => None,
        AgentRecordState::Legacy(_) => Some("legacy".to_string()),
        AgentRecordState::Registered(record) => Some(record.lifecycle_status().to_string()),
    }
}

fn push_audit_entry(report: &mut AgentAuditReport, entry: AgentAuditEntry) {
    match entry.risk_level {
        "critical" => report.critical += 1,
        "warning" => report.warning += 1,
        "info" => report.info += 1,
        _ => report.healthy += 1,
    }
    if entry.action_required {
        report.action_required += 1;
    }
    report.agents.push(entry);
}

pub fn run(args: AgentArgs) -> anyhow::Result<()> {
    let json = args.json;
    match args.command {
        AgentCommand::Register {
            name,
            version,
            public_key,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let requested_version = version.clone();

            let (record, id, created) = match read_agent_record(&store, &name)? {
                AgentRecordState::Registered(mut existing) => {
                    if existing.is_revoked() {
                        anyhow::bail!(
                            "agent '{name}' is revoked; run `claw agent rotate --name {name}` to trust a replacement key"
                        );
                    }
                    if let Some(v) = requested_version.clone() {
                        existing.agent_version = Some(v);
                    }
                    existing.private_key = None;

                    if let Some(public_key) = public_key.clone() {
                        existing.public_key = normalize_public_key(&public_key)?;
                    } else {
                        match ensure_local_key_for_registration(&existing, &name) {
                            Ok(true) => {}
                            Ok(false) | Err(_) => {
                                let keypair = KeyPair::generate();
                                existing.public_key = hex::encode(keypair.public_key_bytes());
                                save_local_agent_key(&name, &keypair)?;
                            }
                        }
                    }
                    existing.updated_at_ms = now_ms()?;
                    let id = store_agent_registration(&store, &name, &existing)?;
                    (existing, id, false)
                }
                AgentRecordState::Legacy(legacy) => {
                    let version = requested_version.clone().or(legacy.agent_version);
                    let record = if let Some(public_key) = public_key.clone() {
                        new_external_registration(&name, version, public_key)?
                    } else {
                        let (record, keypair) = new_registration(&name, version)?;
                        save_local_agent_key(&name, &keypair)?;
                        record
                    };
                    let id = store_agent_registration(&store, &name, &record)?;
                    (record, id, true)
                }
                AgentRecordState::Missing => {
                    let record = if let Some(public_key) = public_key.clone() {
                        new_external_registration(&name, requested_version.clone(), public_key)?
                    } else {
                        let (record, keypair) = new_registration(&name, requested_version.clone())?;
                        save_local_agent_key(&name, &keypair)?;
                        record
                    };
                    let id = store_agent_registration(&store, &name, &record)?;
                    (record, id, true)
                }
            };

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.register",
                        "created": created,
                        "agent_id": record.agent_id,
                        "agent_version": record.agent_version,
                        "public_key": record.public_key,
                        "object_id": id.to_string(),
                        "status": record.lifecycle_status(),
                    }))?
                );
            } else {
                if created {
                    println!("Registered agent: {name}");
                } else {
                    println!("Updated agent: {name}");
                }
                if let Some(v) = record.agent_version.as_deref() {
                    println!("  Version: {v}");
                }
                println!("  Public key: {}", record.public_key_prefix());
                println!("  Object: {id}");
            }
        }
        AgentCommand::Keygen { name, overwrite } => {
            let path = agent_key_path(&name)?;
            if path.exists() && !overwrite {
                anyhow::bail!(
                    "local signing key already exists for agent '{name}'; use --overwrite to replace it"
                );
            }
            let keypair = KeyPair::generate();
            save_local_agent_key(&name, &keypair)?;
            let public_key = hex::encode(keypair.public_key_bytes());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.keygen",
                        "agent_id": name,
                        "public_key": public_key,
                        "key_path": path.display().to_string(),
                    }))?
                );
            } else {
                println!("Generated local agent key: {name}");
                println!("  Public key: {public_key}");
                println!("  Key path: {}", path.display());
            }
        }
        AgentCommand::Rotate {
            name,
            version,
            public_key,
            dry_run,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let current = match read_agent_record(&store, &name)? {
                AgentRecordState::Registered(record) => record,
                AgentRecordState::Legacy(_) => {
                    anyhow::bail!(
                        "agent '{name}' uses a legacy registration; run `claw agent register --name {name}` before rotating"
                    )
                }
                AgentRecordState::Missing => {
                    anyhow::bail!("agent '{name}' is not registered; run `claw agent register --name {name}` first")
                }
            };

            let replacement_version = version.clone().or(current.agent_version.clone());
            let normalized_public_key = public_key
                .as_deref()
                .map(normalize_public_key)
                .transpose()?;
            if dry_run {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.rotate",
                            "dry_run": true,
                            "agent_id": name,
                            "current_public_key": current.public_key,
                            "replacement_public_key": normalized_public_key,
                            "replacement_version": replacement_version,
                            "current_status": current.lifecycle_status(),
                        }))?
                    );
                } else {
                    println!("Dry run: would rotate agent: {name}");
                    println!("  Current public key: {}", current.public_key_prefix());
                    if let Some(public_key) = normalized_public_key.as_deref() {
                        println!(
                            "  Replacement public key: {}",
                            public_key_prefix(public_key)
                        );
                    }
                    if current.is_revoked() {
                        println!("  Current status: revoked");
                    } else if current.is_quarantined() {
                        println!("  Current status: quarantined");
                    }
                    if let Some(v) = replacement_version {
                        println!("  Replacement version: {v}");
                    }
                    println!("  Repository and local key store were not modified.");
                }
                return Ok(());
            }

            let mut rotated = if let Some(public_key) = normalized_public_key {
                new_external_registration(&name, replacement_version, public_key)?
            } else {
                let (record, keypair) = new_registration(&name, replacement_version)?;
                save_local_agent_key(&name, &keypair)?;
                record
            };
            rotated.created_at_ms = current.created_at_ms;
            rotated.updated_at_ms = now_ms()?;
            rotated.revoked_at_ms = None;
            rotated.revocation_reason = None;
            rotated.quarantined_at_ms = None;
            rotated.quarantine_reason = None;
            let id = store_agent_registration(&store, &name, &rotated)?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.rotate",
                        "dry_run": false,
                        "agent_id": rotated.agent_id,
                        "agent_version": rotated.agent_version,
                        "previous_public_key": current.public_key,
                        "public_key": rotated.public_key,
                        "object_id": id.to_string(),
                        "status": "active",
                    }))?
                );
            } else {
                println!("Rotated agent: {name}");
                println!("  Previous public key: {}", current.public_key_prefix());
                println!("  New public key: {}", rotated.public_key_prefix());
                if let Some(v) = rotated.agent_version.as_deref() {
                    println!("  Version: {v}");
                }
                println!("  Object: {id}");
            }
        }
        AgentCommand::Revoke {
            name,
            reason,
            dry_run,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut record = match read_agent_record(&store, &name)? {
                AgentRecordState::Registered(record) => record,
                AgentRecordState::Legacy(_) => {
                    anyhow::bail!(
                        "agent '{name}' uses a legacy registration; re-register before revoking"
                    )
                }
                AgentRecordState::Missing => anyhow::bail!("agent '{name}' is not registered"),
            };

            if dry_run {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.revoke",
                            "dry_run": true,
                            "agent_id": name,
                            "public_key": record.public_key,
                            "reason": reason,
                        }))?
                    );
                } else {
                    println!("Dry run: would revoke agent: {name}");
                    println!("  Public key: {}", record.public_key_prefix());
                    if let Some(reason) = reason.as_deref() {
                        println!("  Reason: {reason}");
                    }
                    println!("  Repository was not modified.");
                }
                return Ok(());
            }

            if record.is_revoked() {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.revoke",
                            "dry_run": false,
                            "changed": false,
                            "agent_id": record.agent_id,
                            "public_key": record.public_key,
                            "revoked_at_ms": record.revoked_at_ms,
                            "reason": record.revocation_reason,
                        }))?
                    );
                } else {
                    println!("Agent already revoked: {name}");
                    if let Some(reason) = record.revocation_reason.as_deref() {
                        println!("  Reason: {reason}");
                    }
                }
                return Ok(());
            }

            record.revoked_at_ms = Some(now_ms()?);
            record.revocation_reason = reason;
            record.updated_at_ms = now_ms()?;
            let id = store_agent_registration(&store, &name, &record)?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.revoke",
                        "dry_run": false,
                        "changed": true,
                        "agent_id": record.agent_id,
                        "public_key": record.public_key,
                        "revoked_at_ms": record.revoked_at_ms,
                        "reason": record.revocation_reason,
                        "object_id": id.to_string(),
                    }))?
                );
            } else {
                println!("Revoked agent: {name}");
                println!("  Public key: {}", record.public_key_prefix());
                if let Some(reason) = record.revocation_reason.as_deref() {
                    println!("  Reason: {reason}");
                }
                println!("  Object: {id}");
            }
        }
        AgentCommand::Quarantine {
            name,
            reason,
            dry_run,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut record = match read_agent_record(&store, &name)? {
                AgentRecordState::Registered(record) => record,
                AgentRecordState::Legacy(_) => {
                    anyhow::bail!(
                        "agent '{name}' uses a legacy registration; re-register before quarantining"
                    )
                }
                AgentRecordState::Missing => anyhow::bail!("agent '{name}' is not registered"),
            };

            if record.is_revoked() {
                anyhow::bail!("agent '{name}' is revoked; rotate the key to create a new trusted registration");
            }

            if dry_run {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.quarantine",
                            "dry_run": true,
                            "agent_id": name,
                            "public_key": record.public_key,
                            "current_status": record.lifecycle_status(),
                            "reason": reason,
                        }))?
                    );
                } else {
                    println!("Dry run: would quarantine agent: {name}");
                    println!("  Public key: {}", record.public_key_prefix());
                    println!("  Current status: {}", record.lifecycle_status());
                    if let Some(reason) = reason.as_deref() {
                        println!("  Reason: {reason}");
                    }
                    println!("  Repository was not modified.");
                }
                return Ok(());
            }

            if record.is_quarantined() {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.quarantine",
                            "dry_run": false,
                            "changed": false,
                            "agent_id": record.agent_id,
                            "public_key": record.public_key,
                            "quarantined_at_ms": record.quarantined_at_ms,
                            "reason": record.quarantine_reason,
                        }))?
                    );
                } else {
                    println!("Agent already quarantined: {name}");
                    if let Some(reason) = record.quarantine_reason.as_deref() {
                        println!("  Reason: {reason}");
                    }
                }
                return Ok(());
            }

            record.quarantined_at_ms = Some(now_ms()?);
            record.quarantine_reason = reason;
            record.updated_at_ms = now_ms()?;
            let id = store_agent_registration(&store, &name, &record)?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.quarantine",
                        "dry_run": false,
                        "changed": true,
                        "agent_id": record.agent_id,
                        "public_key": record.public_key,
                        "quarantined_at_ms": record.quarantined_at_ms,
                        "reason": record.quarantine_reason,
                        "object_id": id.to_string(),
                    }))?
                );
            } else {
                println!("Quarantined agent: {name}");
                println!("  Public key: {}", record.public_key_prefix());
                if let Some(reason) = record.quarantine_reason.as_deref() {
                    println!("  Reason: {reason}");
                }
                println!("  Object: {id}");
            }
        }
        AgentCommand::Unquarantine { name, dry_run } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut record = match read_agent_record(&store, &name)? {
                AgentRecordState::Registered(record) => record,
                AgentRecordState::Legacy(_) => {
                    anyhow::bail!(
                        "agent '{name}' uses a legacy registration; re-register before unquarantining"
                    )
                }
                AgentRecordState::Missing => anyhow::bail!("agent '{name}' is not registered"),
            };

            if record.is_revoked() {
                anyhow::bail!("agent '{name}' is revoked; rotate the key to create a new trusted registration");
            }

            if dry_run {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.unquarantine",
                            "dry_run": true,
                            "agent_id": name,
                            "public_key": record.public_key,
                            "current_status": record.lifecycle_status(),
                        }))?
                    );
                } else {
                    println!("Dry run: would unquarantine agent: {name}");
                    println!("  Public key: {}", record.public_key_prefix());
                    println!("  Current status: {}", record.lifecycle_status());
                    println!("  Repository was not modified.");
                }
                return Ok(());
            }

            if !record.is_quarantined() {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "agent.unquarantine",
                            "dry_run": false,
                            "changed": false,
                            "agent_id": record.agent_id,
                            "public_key": record.public_key,
                            "status": record.lifecycle_status(),
                        }))?
                    );
                } else {
                    println!("Agent is not quarantined: {name}");
                    println!("  Status: {}", record.lifecycle_status());
                }
                return Ok(());
            }

            record.quarantined_at_ms = None;
            record.quarantine_reason = None;
            record.updated_at_ms = now_ms()?;
            let id = store_agent_registration(&store, &name, &record)?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.unquarantine",
                        "dry_run": false,
                        "changed": true,
                        "agent_id": record.agent_id,
                        "public_key": record.public_key,
                        "object_id": id.to_string(),
                        "status": record.lifecycle_status(),
                    }))?
                );
            } else {
                println!("Unquarantined agent: {name}");
                println!("  Public key: {}", record.public_key_prefix());
                println!("  Object: {id}");
            }
        }
        AgentCommand::Bulk { file, dry_run } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let plan = load_bulk_plan(&file)?;
            let report = run_bulk_plan(&store, plan, dry_run)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Agent bulk {}: {} planned, {} {}",
                    if dry_run { "dry run" } else { "apply" },
                    report.planned_count,
                    report.changed_count,
                    if dry_run { "would change" } else { "changed" }
                );
                for result in &report.results {
                    println!(
                        "  {} {} -> {}{}",
                        result.operation,
                        result.agent_id,
                        result.status.as_deref().unwrap_or("unknown"),
                        result
                            .note
                            .as_deref()
                            .map(|note| format!(" ({note})"))
                            .unwrap_or_default()
                    );
                }
            }
        }
        AgentCommand::Audit {
            status,
            risk,
            action_required,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let report = filter_agent_audit_report(
                audit_agent_records(&store)?,
                AgentAuditFilters {
                    status,
                    risk,
                    action_required,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Agent audit");
                println!("  Total refs: {}", report.total_refs);
                println!("  Matching agents: {}", report.matching_agents);
                println!(
                    "  Registered: {} active, {} quarantined, {} revoked",
                    report.active, report.quarantined, report.revoked
                );
                println!("  Legacy: {}", report.legacy);
                println!("  Malformed: {}", report.malformed);
                println!(
                    "  Local keys: {} verified, {} absent, {} mismatched/unreadable",
                    report.local_key_verified, report.local_key_missing, report.local_key_mismatch
                );
                println!(
                    "  Triage: {} healthy, {} action required, {} critical, {} warning, {} info",
                    report.healthy,
                    report.action_required,
                    report.critical,
                    report.warning,
                    report.info
                );
                if report.findings.is_empty() {
                    println!("  Findings: none");
                } else {
                    println!("  Findings:");
                    for finding in &report.findings {
                        println!(
                            "    [{}] {} {}: {}",
                            finding.severity, finding.agent_id, finding.code, finding.message
                        );
                    }
                }
            }
        }
        AgentCommand::Status { name } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;

            if let Some(n) = name {
                match read_agent_record(&store, &n)? {
                    AgentRecordState::Registered(agent) => {
                        let key_ok = registration_keypair(&agent, &n).is_ok();
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "action": "agent.status",
                                    "found": true,
                                    "kind": "registered",
                                    "agent_id": agent.agent_id,
                                    "agent_version": agent.agent_version,
                                    "public_key": agent.public_key,
                                    "key_status": if key_ok { "verified" } else { "invalid" },
                                    "status": agent.lifecycle_status(),
                                    "revoked_at_ms": agent.revoked_at_ms,
                                    "revocation_reason": agent.revocation_reason,
                                    "quarantined_at_ms": agent.quarantined_at_ms,
                                    "quarantine_reason": agent.quarantine_reason,
                                }))?
                            );
                        } else {
                            println!("Agent: {}", agent.agent_id);
                            if let Some(v) = &agent.agent_version {
                                println!("  Version: {v}");
                            }
                            println!(
                                "  Key: {} ({})",
                                agent.public_key_prefix(),
                                if key_ok { "verified" } else { "invalid" }
                            );
                            if agent.is_revoked() {
                                println!("  Status: revoked");
                                if let Some(reason) = agent.revocation_reason.as_deref() {
                                    println!("  Revocation reason: {reason}");
                                }
                            } else if agent.is_quarantined() {
                                println!("  Status: quarantined");
                                if let Some(reason) = agent.quarantine_reason.as_deref() {
                                    println!("  Quarantine reason: {reason}");
                                }
                            } else {
                                println!("  Status: active");
                            }
                        }
                    }
                    AgentRecordState::Legacy(agent) => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "action": "agent.status",
                                    "found": true,
                                    "kind": "legacy",
                                    "agent_id": agent.agent_id,
                                    "agent_version": agent.agent_version,
                                    "key_status": "missing",
                                    "status": "legacy",
                                }))?
                            );
                        } else {
                            println!("Agent: {}", agent.agent_id);
                            if let Some(v) = &agent.agent_version {
                                println!("  Version: {v}");
                            }
                            println!("  Key: missing (legacy registration)");
                            println!("  Status: legacy");
                        }
                    }
                    AgentRecordState::Missing => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "action": "agent.status",
                                    "found": false,
                                    "agent_id": n,
                                }))?
                            );
                        } else {
                            println!("Agent {n}: not found");
                        }
                    }
                }
            } else if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.status",
                        "error": "missing_agent_name",
                        "remediation": "Use `claw agent list --json` to see all agents.",
                    }))?
                );
            } else {
                println!("Use 'claw agent list' to see all agents.");
            }
        }
        AgentCommand::List => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("agents")?;
            if json {
                let mut agents = Vec::new();
                for (name, id) in &refs {
                    let agent_name = name.trim_start_matches("agents/");
                    match read_agent_record(&store, agent_name) {
                        Ok(AgentRecordState::Registered(agent)) => {
                            let key_ok = registration_keypair(&agent, agent_name).is_ok();
                            agents.push(serde_json::json!({
                                "agent_id": agent.agent_id,
                                "agent_version": agent.agent_version,
                                "public_key": agent.public_key,
                                "key_status": if key_ok { "verified" } else { "invalid" },
                                "status": agent.lifecycle_status(),
                                "revoked_at_ms": agent.revoked_at_ms,
                                "revocation_reason": agent.revocation_reason,
                                "quarantined_at_ms": agent.quarantined_at_ms,
                                "quarantine_reason": agent.quarantine_reason,
                                "object_id": id.to_string(),
                            }));
                        }
                        Ok(AgentRecordState::Legacy(agent)) => {
                            agents.push(serde_json::json!({
                                "agent_id": agent.agent_id,
                                "agent_version": agent.agent_version,
                                "key_status": "legacy",
                                "status": "legacy",
                                "object_id": id.to_string(),
                            }));
                        }
                        Ok(AgentRecordState::Missing) | Err(_) => {
                            agents.push(serde_json::json!({
                                "ref": name,
                                "object_id": id.to_string(),
                                "status": "unreadable",
                            }));
                        }
                    }
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "agent.list",
                        "agent_count": agents.len(),
                        "agents": agents,
                    }))?
                );
            } else if refs.is_empty() {
                println!("No agents registered.");
            } else {
                for (name, id) in &refs {
                    let agent_name = name.trim_start_matches("agents/");
                    match read_agent_record(&store, agent_name) {
                        Ok(AgentRecordState::Registered(agent)) => {
                            let key_ok = registration_keypair(&agent, agent_name).is_ok();
                            println!(
                                "{} v{} key:{} status:{}",
                                agent.agent_id,
                                agent.agent_version.as_deref().unwrap_or("?"),
                                if key_ok { "verified" } else { "invalid" },
                                if agent.is_revoked() {
                                    "revoked"
                                } else if agent.is_quarantined() {
                                    "quarantined"
                                } else {
                                    "active"
                                }
                            );
                        }
                        Ok(AgentRecordState::Legacy(agent)) => {
                            println!(
                                "{} v{} key:legacy",
                                agent.agent_id,
                                agent.agent_version.as_deref().unwrap_or("?")
                            );
                        }
                        Ok(AgentRecordState::Missing) => {
                            println!("{name}");
                        }
                        Err(_) => {
                            if store.load_object(id).is_ok() {
                                println!("{name}");
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{audit_triage, AgentArgs, AgentCommand, AgentRegistration};

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: AgentArgs,
    }

    #[test]
    fn parses_rotate_dry_run() {
        let cli = TestCli::parse_from([
            "claw",
            "rotate",
            "--name",
            "ci-agent",
            "--version",
            "2026-05-11",
            "--dry-run",
        ]);

        match cli.args.command {
            AgentCommand::Rotate {
                name,
                version,
                public_key,
                dry_run,
            } => {
                assert_eq!(name, "ci-agent");
                assert_eq!(version.as_deref(), Some("2026-05-11"));
                assert!(public_key.is_none());
                assert!(dry_run);
            }
            _ => panic!("expected rotate command"),
        }
    }

    #[test]
    fn parses_keygen_overwrite() {
        let cli = TestCli::parse_from(["claw", "keygen", "--name", "ci-agent", "--overwrite"]);

        match cli.args.command {
            AgentCommand::Keygen { name, overwrite } => {
                assert_eq!(name, "ci-agent");
                assert!(overwrite);
            }
            _ => panic!("expected keygen command"),
        }
    }

    #[test]
    fn parses_json_after_subcommand() {
        let cli = TestCli::parse_from(["claw", "list", "--json"]);

        assert!(cli.args.json);
        assert!(matches!(cli.args.command, AgentCommand::List));
    }

    #[test]
    fn parses_register_public_key() {
        let public_key = "aa".repeat(32);
        let cli = TestCli::parse_from([
            "claw",
            "register",
            "--name",
            "ci-agent",
            "--public-key",
            &public_key,
        ]);

        match cli.args.command {
            AgentCommand::Register {
                name,
                public_key: parsed_public_key,
                ..
            } => {
                assert_eq!(name, "ci-agent");
                assert_eq!(parsed_public_key.as_deref(), Some(public_key.as_str()));
            }
            _ => panic!("expected register command"),
        }
    }

    #[test]
    fn parses_revoke_reason_and_dry_run() {
        let cli = TestCli::parse_from([
            "claw",
            "revoke",
            "--name",
            "ci-agent",
            "--reason",
            "compromised",
            "--dry-run",
        ]);

        match cli.args.command {
            AgentCommand::Revoke {
                name,
                reason,
                dry_run,
            } => {
                assert_eq!(name, "ci-agent");
                assert_eq!(reason.as_deref(), Some("compromised"));
                assert!(dry_run);
            }
            _ => panic!("expected revoke command"),
        }
    }

    #[test]
    fn parses_quarantine_reason_and_dry_run() {
        let cli = TestCli::parse_from([
            "claw",
            "quarantine",
            "--name",
            "ci-agent",
            "--reason",
            "runner drift",
            "--dry-run",
        ]);

        match cli.args.command {
            AgentCommand::Quarantine {
                name,
                reason,
                dry_run,
            } => {
                assert_eq!(name, "ci-agent");
                assert_eq!(reason.as_deref(), Some("runner drift"));
                assert!(dry_run);
            }
            _ => panic!("expected quarantine command"),
        }
    }

    #[test]
    fn parses_unquarantine_dry_run() {
        let cli = TestCli::parse_from(["claw", "unquarantine", "--name", "ci-agent", "--dry-run"]);

        match cli.args.command {
            AgentCommand::Unquarantine { name, dry_run } => {
                assert_eq!(name, "ci-agent");
                assert!(dry_run);
            }
            _ => panic!("expected unquarantine command"),
        }
    }

    #[test]
    fn parses_bulk_dry_run() {
        let cli = TestCli::parse_from(["claw", "bulk", "--file", "agents.json", "--dry-run"]);

        match cli.args.command {
            AgentCommand::Bulk { file, dry_run } => {
                assert_eq!(file, std::path::PathBuf::from("agents.json"));
                assert!(dry_run);
            }
            _ => panic!("expected bulk command"),
        }
    }

    #[test]
    fn parses_audit_filters() {
        let cli = TestCli::parse_from([
            "claw",
            "audit",
            "--status",
            "revoked",
            "--risk",
            "warning",
            "--action-required",
        ]);

        match cli.args.command {
            AgentCommand::Audit {
                status,
                risk,
                action_required,
            } => {
                assert_eq!(status.as_deref(), Some("revoked"));
                assert_eq!(risk.as_deref(), Some("warning"));
                assert!(action_required);
            }
            _ => panic!("expected audit command"),
        }
    }

    #[test]
    fn registration_revocation_defaults_to_active_for_legacy_json() {
        let json = r#"{
            "schema_version": 2,
            "agent_id": "ci-agent",
            "public_key": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "created_at_ms": 1,
            "updated_at_ms": 1
        }"#;

        let record: AgentRegistration = serde_json::from_str(json).unwrap();
        assert!(!record.is_revoked());
        assert!(!record.is_quarantined());
        assert!(record.revocation_reason.is_none());
        assert!(record.quarantine_reason.is_none());
    }

    #[test]
    fn audit_triage_prioritizes_fleet_actions() {
        let healthy = audit_triage("registered", "active", "verified", false);
        assert_eq!(healthy.risk_level, "none");
        assert!(!healthy.action_required);
        assert_eq!(healthy.recommended_action, "none");

        let quarantined = audit_triage("registered", "quarantined", "verified", false);
        assert_eq!(quarantined.risk_level, "warning");
        assert!(quarantined.action_required);
        assert_eq!(
            quarantined.recommended_action,
            "review_quarantine_then_rotate_or_unquarantine"
        );

        let malformed = audit_triage("malformed", "active", "not_checked", false);
        assert_eq!(malformed.risk_level, "critical");
        assert!(malformed.action_required);
        assert_eq!(
            malformed.recommended_action,
            "repair_or_reregister_agent_record"
        );

        let external_key = audit_triage("registered", "active", "missing", false);
        assert_eq!(external_key.risk_level, "info");
        assert!(!external_key.action_required);
        assert_eq!(
            external_key.recommended_action,
            "confirm_external_key_management_or_provision_local_key"
        );
    }
}
