//! Rust helpers for agents that drive the `claw` CLI.
//!
//! The SDK keeps process orchestration, JSON parsing, and MCP message shapes in
//! one small crate so agent integrations do not need to duplicate shell glue.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result alias for the Rust agent SDK.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the Rust agent SDK.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `claw` process could not be launched.
    #[error("failed to launch claw: {0}")]
    Io(#[from] std::io::Error),
    /// The command exited with a non-zero status.
    #[error("claw exited with status {status}: {stderr}")]
    Command {
        /// Process exit status code, or -1 when unavailable.
        status: i32,
        /// Captured standard error.
        stderr: String,
        /// Captured standard output.
        stdout: String,
    },
    /// Successful command output was not valid JSON.
    #[error("claw emitted invalid json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Builder for running Claw commands from an agent process.
#[derive(Clone, Debug)]
pub struct ClawClient {
    binary: PathBuf,
    cwd: Option<PathBuf>,
}

/// Input for creating an intent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IntentCreate {
    /// Short intent title.
    pub title: String,
    /// Goal statement for the intent.
    pub goal: String,
    /// Runnable acceptance commands linked to the intent.
    pub acceptance_tests: Vec<String>,
}

/// Input for running executable acceptance tests linked to an intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceRun {
    /// Intent ID to run.
    pub intent_id: String,
    /// Per-command timeout in milliseconds.
    pub timeout_ms: u64,
    /// Continue after a failed command.
    pub keep_going: bool,
}

impl Default for AcceptanceRun {
    fn default() -> Self {
        Self {
            intent_id: String::new(),
            timeout_ms: 300_000,
            keep_going: false,
        }
    }
}

/// Input for querying evidence.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EvidenceQuery {
    /// Boolean evidence query expression.
    pub query: String,
    /// Optional revision filter.
    pub revision: Option<String>,
    /// Optional capsule filter.
    pub capsule: Option<String>,
    /// Optional result limit.
    pub limit: Option<usize>,
}

/// Input for querying agent fleet audit results.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentAuditRequest {
    /// Optional lifecycle status filter.
    pub status: Option<String>,
    /// Optional risk level filter.
    pub risk: Option<String>,
    /// Only include agents requiring operator action.
    pub action_required: bool,
}

/// Input for applying or previewing an agent fleet bulk plan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentBulkRequest {
    /// JSON plan file path.
    pub file: String,
    /// Validate and report the ordered plan without writing refs or local keys.
    pub dry_run: bool,
}

/// Input for repository health checks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DoctorRequest {
    /// Run the deep object graph, capsule, policy, and repairability scan.
    pub deep: bool,
    /// Return a non-zero status when doctor finds errors.
    pub strict: bool,
}

/// Input for exporting a human-readable story as structured JSON.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StoryExportRequest {
    /// Intent ID to export.
    pub intent: String,
}

/// Input for timeline allowance debugging.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TimelineAllowedRequest {
    /// Revision ref or object id.
    pub revision: String,
    /// Policy ids/refs to evaluate. Defaults to all policies when empty.
    pub policies: Vec<String>,
    /// Verified signer agent ids.
    pub signer_agents: Vec<String>,
    /// Verified signer key ids.
    pub signer_keys: Vec<String>,
    /// Touched paths for sensitive path checks.
    pub paths: Vec<String>,
}

/// Input for patch algebra workbench.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatchWorkbenchRequest {
    /// Left patch object/ref.
    pub left: String,
    /// Right patch object/ref.
    pub right: String,
}

/// Input for policy simulation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolicySimulateRequest {
    /// Stored policy id/ref. Omit when `policy_file` is set.
    pub policy_id: Option<String>,
    /// Candidate policy JSON/TOML file to evaluate without storing it.
    pub policy_file: Option<String>,
    /// Revision ref or object id.
    pub revision: String,
    /// Optional capsule ref or object id. Defaults to the revision capsule.
    pub capsule: Option<String>,
    /// Verified signer agent ids.
    pub signer_agents: Vec<String>,
    /// Verified signer key ids.
    pub signer_keys: Vec<String>,
    /// Touched paths for sensitive-path checks.
    pub paths: Vec<String>,
    /// Optional trust score override.
    pub trust_score: Option<String>,
}

/// Input for review reports.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReviewRequest {
    /// Optional intent selector.
    pub intent: Option<String>,
    /// Optional change selector.
    pub change: Option<String>,
    /// Optional capsule selector.
    pub capsule: Option<String>,
}

/// Input for trust receipts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrustReceiptRequest {
    /// Revision ref or object id.
    pub revision: String,
    /// Optional capsule ref or object id.
    pub capsule: Option<String>,
    /// Policy ids/refs to evaluate.
    pub policies: Vec<String>,
    /// Verified signer agent ids.
    pub signer_agents: Vec<String>,
    /// Verified signer key ids.
    pub signer_keys: Vec<String>,
    /// Touched paths for sensitive path checks.
    pub paths: Vec<String>,
    /// Optional trust score override.
    pub trust_score: Option<String>,
}

/// Input for shipping a revision.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShipRequest {
    /// Intent ID to ship.
    pub intent: String,
    /// Revision ref to ship, defaults to `heads/main` when omitted.
    pub revision_ref: Option<String>,
    /// Signing agent id.
    pub agent: Option<String>,
    /// Evidence items in `name=status[:duration_ms]` form.
    pub evidence: Vec<String>,
    /// Command that produced the evidence.
    pub evidence_command: Option<String>,
    /// Runner identity.
    pub runner: Option<String>,
    /// Environment digest.
    pub environment_digest: Option<String>,
    /// Log digest.
    pub log_digest: Option<String>,
    /// Artifact digest.
    pub artifact_digest: Option<String>,
    /// Evidence TTL in milliseconds.
    pub evidence_expires_in_ms: Option<u64>,
    /// Private capsule metadata file.
    pub private_file: Option<String>,
    /// Recipient keys in `recipient-id:key-id:hex-x25519-public-key` form.
    pub recipient_keys: Vec<String>,
    /// Additional co-signing agents.
    pub co_signers: Vec<String>,
}

/// Input for provenance replay.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProvenanceReplayRequest {
    /// Revision ref or object id.
    pub revision: String,
    /// Optional capsule ref or object id.
    pub capsule: Option<String>,
    /// Evidence names to replay.
    pub evidence_names: Vec<String>,
    /// Signing agent id.
    pub agent: Option<String>,
    /// Timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Continue after mismatch.
    pub keep_going: bool,
    /// Run in repository root instead of sandbox.
    pub in_place: bool,
    /// Preview without storing a replacement capsule.
    pub dry_run: bool,
}

/// Input for attaching SLSA/in-toto attestations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AttestationRequest {
    /// Revision ref or object id.
    pub revision: String,
    /// Optional capsule ref or object id.
    pub capsule: Option<String>,
    /// Attestation JSON file path.
    pub file: String,
    /// Signing agent id.
    pub agent: Option<String>,
    /// Expected subject name.
    pub subject_name: Option<String>,
    /// Expected subject digest.
    pub subject_digest: Option<String>,
    /// Expected SLSA builder.id value.
    pub builder_id: Option<String>,
    /// Expected SLSA predicate buildType value.
    pub build_type: Option<String>,
    /// Preview without storing a replacement capsule.
    pub dry_run: bool,
}

impl Default for ClawClient {
    fn default() -> Self {
        Self::new("claw")
    }
}

impl ClawClient {
    /// Create a client that executes the given `claw` binary.
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            cwd: None,
        }
    }

    /// Run commands from a specific repository directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Return the configured binary path.
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Run `claw <args>` and parse stdout as JSON.
    pub fn json<I, S>(&self, args: I) -> Result<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output(args)?;
        Ok(serde_json::from_str(&output.stdout)?)
    }

    /// Run `claw status --json`.
    pub fn status(&self) -> Result<Value> {
        self.json(["status", "--json"])
    }

    /// Run `claw intent --json list`.
    pub fn list_intents(&self) -> Result<Value> {
        self.json(["intent", "--json", "list"])
    }

    /// Run `claw intent --json graph`.
    pub fn intent_graph(&self) -> Result<Value> {
        self.json(["intent", "--json", "graph"])
    }

    /// Run `claw intent --json create --title <title> --goal <goal>`.
    pub fn create_intent(&self, title: &str, goal: &str) -> Result<Value> {
        self.create_intent_with(IntentCreate {
            title: title.to_string(),
            goal: goal.to_string(),
            acceptance_tests: Vec::new(),
        })
    }

    /// Run `claw intent --json create` with typed intent input.
    pub fn create_intent_with(&self, input: IntentCreate) -> Result<Value> {
        let mut args = os_args(&[
            "intent",
            "--json",
            "create",
            "--title",
            &input.title,
            "--goal",
            &input.goal,
        ]);
        for test in input.acceptance_tests {
            args.push("--acceptance-test".into());
            args.push(test.into());
        }
        self.json(args)
    }

    /// Run `claw change --json create --intent <intent_id>`.
    pub fn create_change(&self, intent_id: &str) -> Result<Value> {
        self.json(["change", "--json", "create", "--intent", intent_id])
    }

    /// Run acceptance tests linked to an intent.
    pub fn run_acceptance(&self, input: AcceptanceRun) -> Result<Value> {
        let mut args = os_args(&[
            "intent",
            "--json",
            "run-acceptance",
            &input.intent_id,
            "--timeout-ms",
            &input.timeout_ms.to_string(),
        ]);
        if input.keep_going {
            args.push("--keep-going".into());
        }
        self.json(args)
    }

    /// Run `claw evidence --json query <query>`.
    pub fn query_evidence(&self, query: &str) -> Result<Value> {
        self.query_evidence_with(EvidenceQuery {
            query: query.to_string(),
            ..EvidenceQuery::default()
        })
    }

    /// Run `claw evidence --json query` with typed filters.
    pub fn query_evidence_with(&self, input: EvidenceQuery) -> Result<Value> {
        let mut args = os_args(&["evidence", "--json", "query", &input.query]);
        push_option(&mut args, "--revision", input.revision);
        push_option(&mut args, "--capsule", input.capsule);
        if let Some(limit) = input.limit {
            args.push("--limit".into());
            args.push(limit.to_string().into());
        }
        self.json(args)
    }

    /// Run `claw doctor --json`.
    pub fn doctor(&self, input: DoctorRequest) -> Result<Value> {
        let mut args = os_args(&["doctor", "--json"]);
        if input.deep {
            args.push("--deep".into());
        }
        if input.strict {
            args.push("--strict".into());
        }
        self.json(args)
    }

    /// Run `claw doctor --json --deep`.
    pub fn doctor_deep(&self) -> Result<Value> {
        self.doctor(DoctorRequest {
            deep: true,
            strict: false,
        })
    }

    /// Run `claw repair --json plan`.
    pub fn repair_plan(&self) -> Result<Value> {
        self.json(["repair", "--json", "plan"])
    }

    /// Run `claw agent --json audit` with optional fleet filters.
    pub fn agent_audit(&self, input: AgentAuditRequest) -> Result<Value> {
        let mut args = os_args(&["agent", "--json", "audit"]);
        push_option(&mut args, "--status", input.status);
        push_option(&mut args, "--risk", input.risk);
        if input.action_required {
            args.push("--action-required".into());
        }
        self.json(args)
    }

    /// Run `claw agent --json bulk --file <plan>`.
    pub fn agent_bulk(&self, input: AgentBulkRequest) -> Result<Value> {
        let mut args = os_args(&["agent", "--json", "bulk", "--file", &input.file]);
        if input.dry_run {
            args.push("--dry-run".into());
        }
        self.json(args)
    }

    /// Run `claw story export --format json`.
    pub fn story_export(&self, input: StoryExportRequest) -> Result<Value> {
        self.json([
            "story",
            "export",
            "--intent",
            &input.intent,
            "--format",
            "json",
        ])
    }

    /// Run `claw timeline --json allowed`.
    pub fn timeline_allowed(&self, input: TimelineAllowedRequest) -> Result<Value> {
        let mut args = os_args(&[
            "timeline",
            "--json",
            "allowed",
            "--revision",
            &input.revision,
        ]);
        push_repeated(&mut args, "--policy", input.policies);
        push_repeated(&mut args, "--signer-agent", input.signer_agents);
        push_repeated(&mut args, "--signer-key", input.signer_keys);
        push_repeated(&mut args, "--path", input.paths);
        self.json(args)
    }

    /// Run `claw patch --json workbench`.
    pub fn patch_workbench(&self, input: PatchWorkbenchRequest) -> Result<Value> {
        self.json([
            "patch",
            "--json",
            "workbench",
            "--left",
            &input.left,
            "--right",
            &input.right,
        ])
    }

    /// Run `claw policy simulate`.
    pub fn policy_simulate(&self, input: PolicySimulateRequest) -> Result<Value> {
        let mut args = os_args(&["policy", "simulate"]);
        if let Some(policy_file) = input.policy_file {
            args.push("--policy-file".into());
            args.push(policy_file.into());
        } else if let Some(policy_id) = input.policy_id {
            args.push(policy_id.into());
        }
        args.push("--revision".into());
        args.push(input.revision.into());
        push_option(&mut args, "--capsule", input.capsule);
        push_repeated(&mut args, "--signer-agent", input.signer_agents);
        push_repeated(&mut args, "--signer-key", input.signer_keys);
        push_repeated(&mut args, "--path", input.paths);
        push_option(&mut args, "--trust-score", input.trust_score);
        args.push("--json".into());
        self.json(args)
    }

    /// Run `claw review --json`.
    pub fn review(&self, input: ReviewRequest) -> Result<Value> {
        let mut args = os_args(&["review", "--json"]);
        push_option(&mut args, "--intent", input.intent);
        push_option(&mut args, "--change", input.change);
        push_option(&mut args, "--capsule", input.capsule);
        self.json(args)
    }

    /// Run `claw capsule --json inspect <target>`.
    pub fn inspect_capsule(&self, target: &str) -> Result<Value> {
        self.json(["capsule", "--json", "inspect", target])
    }

    /// Run `claw trust --json receipt`.
    pub fn trust_receipt(&self, input: TrustReceiptRequest) -> Result<Value> {
        let mut args = os_args(&["trust", "--json", "receipt", "--revision", &input.revision]);
        push_option(&mut args, "--capsule", input.capsule);
        push_repeated(&mut args, "--policy", input.policies);
        push_repeated(&mut args, "--signer-agent", input.signer_agents);
        push_repeated(&mut args, "--signer-key", input.signer_keys);
        push_repeated(&mut args, "--path", input.paths);
        push_option(&mut args, "--trust-score", input.trust_score);
        self.json(args)
    }

    /// Run `claw ship --json`.
    pub fn ship(&self, input: ShipRequest) -> Result<Value> {
        let mut args = os_args(&["ship", "--json", "--intent", &input.intent]);
        push_option(&mut args, "--revision-ref", input.revision_ref);
        push_option(&mut args, "--agent", input.agent);
        push_repeated(&mut args, "--evidence", input.evidence);
        push_option(&mut args, "--evidence-command", input.evidence_command);
        push_option(&mut args, "--runner", input.runner);
        push_option(&mut args, "--environment-digest", input.environment_digest);
        push_option(&mut args, "--log-digest", input.log_digest);
        push_option(&mut args, "--artifact-digest", input.artifact_digest);
        if let Some(ttl) = input.evidence_expires_in_ms {
            args.push("--evidence-expires-in-ms".into());
            args.push(ttl.to_string().into());
        }
        push_option(&mut args, "--private-file", input.private_file);
        push_repeated(&mut args, "--recipient-key", input.recipient_keys);
        push_repeated(&mut args, "--co-sign", input.co_signers);
        self.json(args)
    }

    /// Run `claw provenance --json replay`.
    pub fn provenance_replay(&self, input: ProvenanceReplayRequest) -> Result<Value> {
        let mut args = os_args(&[
            "provenance",
            "--json",
            "replay",
            "--revision",
            &input.revision,
        ]);
        push_option(&mut args, "--capsule", input.capsule);
        push_repeated(&mut args, "--evidence", input.evidence_names);
        push_option(&mut args, "--agent", input.agent);
        if let Some(timeout_ms) = input.timeout_ms {
            args.push("--timeout-ms".into());
            args.push(timeout_ms.to_string().into());
        }
        if input.keep_going {
            args.push("--keep-going".into());
        }
        if input.in_place {
            args.push("--in-place".into());
        }
        if input.dry_run {
            args.push("--dry-run".into());
        }
        self.json(args)
    }

    /// Run `claw provenance --json attach-attestation`.
    pub fn attach_attestation(&self, input: AttestationRequest) -> Result<Value> {
        let mut args = os_args(&[
            "provenance",
            "--json",
            "attach-attestation",
            "--revision",
            &input.revision,
            "--file",
            &input.file,
        ]);
        push_option(&mut args, "--capsule", input.capsule);
        push_option(&mut args, "--agent", input.agent);
        push_option(&mut args, "--subject-name", input.subject_name);
        push_option(&mut args, "--subject-digest", input.subject_digest);
        push_option(&mut args, "--builder-id", input.builder_id);
        push_option(&mut args, "--build-type", input.build_type);
        if input.dry_run {
            args.push("--dry-run".into());
        }
        self.json(args)
    }

    /// Run `claw <args>` and capture UTF-8 stdout/stderr.
    pub fn output<I, S>(&self, args: I) -> Result<CommandOutput>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.binary);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        for arg in args {
            command.arg(arg);
        }
        let output = command.output()?;
        let status = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            return Err(Error::Command {
                status,
                stderr,
                stdout,
            });
        }
        Ok(CommandOutput {
            status,
            stdout,
            stderr,
        })
    }
}

/// Captured output from a `claw` invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    /// Process exit status code.
    pub status: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

/// JSON-RPC request shape used by the Claw MCP stdio server.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct McpRequest {
    /// JSON-RPC version, normally `2.0`.
    pub jsonrpc: String,
    /// Method name.
    pub method: String,
    /// Request id. Notifications omit this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Method parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// Convert string slices into owned OS strings for dynamic command assembly.
pub fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn push_option(args: &mut Vec<OsString>, flag: &str, value: Option<String>) {
    if let Some(value) = value {
        args.push(flag.into());
        args.push(value.into());
    }
}

fn push_repeated(args: &mut Vec<OsString>, flag: &str, values: Vec<String>) {
    for value in values {
        args.push(flag.into());
        args.push(value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        os_args, AgentAuditRequest, AgentBulkRequest, ClawClient, DoctorRequest, IntentCreate,
        McpRequest, PatchWorkbenchRequest, StoryExportRequest, TimelineAllowedRequest,
    };

    #[test]
    fn client_defaults_to_claw_binary() {
        let client = ClawClient::default();
        assert_eq!(client.binary().to_string_lossy(), "claw");
    }

    #[test]
    fn mcp_request_roundtrips() {
        let request = McpRequest {
            jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            id: Some(serde_json::json!(1)),
            params: None,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: McpRequest = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn os_args_builds_owned_arguments() {
        let args = os_args(&["status", "--json"]);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_string_lossy(), "status");
    }

    #[test]
    fn typed_intent_input_carries_acceptance_tests() {
        let input = IntentCreate {
            title: "Fix cache".to_string(),
            goal: "Make invalidation explicit".to_string(),
            acceptance_tests: vec!["cargo test".to_string()],
        };
        assert_eq!(input.acceptance_tests[0], "cargo test");
    }

    #[test]
    fn typed_doctor_input_carries_deep_health_options() {
        let input = DoctorRequest {
            deep: true,
            strict: true,
        };
        assert!(input.deep);
        assert!(input.strict);
    }

    #[test]
    fn typed_agent_fleet_inputs_carry_filters_and_plan() {
        let audit = AgentAuditRequest {
            status: Some("revoked".to_string()),
            risk: Some("warning".to_string()),
            action_required: true,
        };
        let bulk = AgentBulkRequest {
            file: "agents.json".to_string(),
            dry_run: true,
        };

        assert_eq!(audit.status.as_deref(), Some("revoked"));
        assert_eq!(audit.risk.as_deref(), Some("warning"));
        assert!(audit.action_required);
        assert_eq!(bulk.file, "agents.json");
        assert!(bulk.dry_run);
    }

    #[test]
    fn typed_reasoning_inputs_carry_selectors() {
        let story = StoryExportRequest {
            intent: "01HINTENT".to_string(),
        };
        let timeline = TimelineAllowedRequest {
            revision: "heads/main".to_string(),
            policies: vec!["release".to_string()],
            paths: vec!["src/lib.rs".to_string()],
            ..TimelineAllowedRequest::default()
        };
        let workbench = PatchWorkbenchRequest {
            left: "clw_left".to_string(),
            right: "clw_right".to_string(),
        };
        assert_eq!(story.intent, "01HINTENT");
        assert_eq!(timeline.policies, vec!["release"]);
        assert_eq!(timeline.paths, vec!["src/lib.rs"]);
        assert_eq!(workbench.left, "clw_left");
        assert_eq!(workbench.right, "clw_right");
    }
}
