use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use clap::{Args, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

use claw_core::object::Object;
use claw_core::types::{Capsule, Evidence};
use claw_crypto::capsule::append_capsule_signature;
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::agent::{ensure_registered_signing_agent, keypair_for_agent};
use super::object_refs::{current_time_ms, load_capsule, load_default_capsule, load_revision};

const PROVENANCE_JSON_SCHEMA_VERSION: u8 = 1;

#[derive(Args)]
pub struct ProvenanceArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: ProvenanceCommand,
}

#[derive(Subcommand)]
enum ProvenanceCommand {
    /// Re-run command evidence and attach replay comparison evidence to a capsule
    Replay {
        /// Revision ref, hex ID, or clw_ display ID
        #[arg(long)]
        revision: String,
        /// Capsule ref or object ID. Defaults to the revision capsule.
        #[arg(long)]
        capsule: Option<String>,
        /// Replay only evidence with this name. Repeatable.
        #[arg(long = "evidence")]
        evidence_names: Vec<String>,
        /// Agent that signs the replay capsule.
        #[arg(long, default_value = "claw")]
        agent: String,
        /// Maximum runtime for each replayed command.
        #[arg(long, default_value_t = 300_000)]
        timeout_ms: u64,
        /// Continue replaying later evidence after a mismatch.
        #[arg(long)]
        keep_going: bool,
        /// Replay directly in the repository root instead of an isolated worktree copy.
        #[arg(long)]
        in_place: bool,
        /// Preview replay without storing a replacement capsule.
        #[arg(long)]
        dry_run: bool,
    },
    /// Attach a SLSA or in-toto attestation as signed capsule evidence
    AttachAttestation {
        /// Revision ref, hex ID, or clw_ display ID
        #[arg(long)]
        revision: String,
        /// Capsule ref or object ID. Defaults to the revision capsule.
        #[arg(long)]
        capsule: Option<String>,
        /// Attestation JSON file.
        #[arg(long)]
        file: PathBuf,
        /// Agent that signs the updated capsule.
        #[arg(long, default_value = "claw")]
        agent: String,
        /// Expected in-toto subject name.
        #[arg(long)]
        subject_name: Option<String>,
        /// Expected subject digest in algorithm:value or raw sha256 hex form.
        #[arg(long)]
        subject_digest: Option<String>,
        /// Expected SLSA builder.id value.
        #[arg(long)]
        builder_id: Option<String>,
        /// Expected SLSA predicate buildType value.
        #[arg(long)]
        build_type: Option<String>,
        /// Preview attachment without storing a replacement capsule.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Serialize)]
struct ReplayRow {
    name: String,
    command: String,
    expected_status: String,
    expected_exit_code: Option<i32>,
    expected_log_digest: Option<String>,
    actual_status: String,
    actual_exit_code: Option<i32>,
    actual_log_digest: String,
    log_digest_matched: Option<bool>,
    matched: bool,
    started_at_ms: u64,
    ended_at_ms: u64,
    duration_ms: u64,
    timed_out: bool,
    workspace: String,
}

#[derive(Debug)]
struct ReplayOutcome {
    row: ReplayRow,
    workspace: ReplayWorkspaceKind,
}

#[derive(Debug, Serialize)]
struct AttachmentResult {
    dry_run: bool,
    source_capsule: String,
    updated_capsule: Option<String>,
    revision: String,
    evidence_added: usize,
}

pub fn run(args: ProvenanceArgs) -> anyhow::Result<()> {
    match args.command {
        ProvenanceCommand::Replay {
            revision,
            capsule,
            evidence_names,
            agent,
            timeout_ms,
            keep_going,
            in_place,
            dry_run,
        } => run_replay(ReplayRequest {
            revision,
            capsule,
            evidence_names,
            agent,
            timeout_ms,
            keep_going,
            in_place,
            dry_run,
            json: args.json,
        }),
        ProvenanceCommand::AttachAttestation {
            revision,
            capsule,
            file,
            agent,
            subject_name,
            subject_digest,
            builder_id,
            build_type,
            dry_run,
        } => run_attach_attestation(AttestationRequest {
            revision,
            capsule,
            file,
            agent,
            subject_name,
            subject_digest,
            builder_id,
            build_type,
            dry_run,
            json: args.json,
        }),
    }
}

struct ReplayRequest {
    revision: String,
    capsule: Option<String>,
    evidence_names: Vec<String>,
    agent: String,
    timeout_ms: u64,
    keep_going: bool,
    in_place: bool,
    dry_run: bool,
    json: bool,
}

fn run_replay(request: ReplayRequest) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let (revision_id, revision) = load_revision(&store, &request.revision)?;
    let (capsule_id, capsule) = match request.capsule.as_deref() {
        Some(value) => load_capsule(&store, value)?,
        None => load_default_capsule(&store, &revision_id, &revision)?,
    };
    if capsule.revision_id != revision_id {
        anyhow::bail!(
            "capsule {} is bound to revision {}, not {}",
            capsule_id.to_hex(),
            capsule.revision_id.to_hex(),
            revision_id.to_hex()
        );
    }

    let selected = capsule
        .public_fields
        .evidence
        .iter()
        .filter(|item| item.command.is_some())
        .filter(|item| {
            request.evidence_names.is_empty()
                || request
                    .evidence_names
                    .iter()
                    .any(|name| item.name.eq_ignore_ascii_case(name))
        })
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        anyhow::bail!("no command evidence matched replay filters");
    }
    let selected_count = selected.len();

    let replay_workspace = ReplayWorkspace::prepare(&root, request.in_place)?;
    let mut outcomes = Vec::new();
    let mut replay_claims = Vec::new();
    for item in selected {
        let outcome = replay_one(
            replay_workspace.path(),
            replay_workspace.kind(),
            &item,
            request.timeout_ms,
        )?;
        let evidence = replay_evidence(&capsule_id, &item, &outcome);
        let matched = outcome.row.matched;
        replay_claims.push(evidence);
        outcomes.push(outcome.row);
        if !matched && !request.keep_going {
            break;
        }
    }
    let matched_count = outcomes.iter().filter(|row| row.matched).count();
    let mismatched_count = outcomes.len().saturating_sub(matched_count);

    let result = attach_evidence(
        &store,
        &capsule_id,
        capsule,
        &request.agent,
        replay_claims,
        request.dry_run,
    )?;
    if request.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": PROVENANCE_JSON_SCHEMA_VERSION,
                "action": "provenance.replay",
                "replay": {
                    "revision": revision_id.to_hex(),
                    "capsule": capsule_id.to_hex(),
                    "workspace": replay_workspace.kind().as_str(),
                    "sandboxed": replay_workspace.kind() == ReplayWorkspaceKind::Sandbox,
                    "selected_count": selected_count,
                    "replayed_count": outcomes.len(),
                    "matched_count": matched_count,
                    "mismatched_count": mismatched_count,
                    "matched": outcomes.iter().all(|row| row.matched),
                    "results": outcomes,
                },
                "attachment": result,
            }))?
        );
    } else {
        println!(
            "Replayed {} evidence command(s) for revision {}.",
            outcomes.len(),
            revision_id.to_hex(),
        );
        println!("  Workspace: {}", replay_workspace.kind().as_str());
        for row in &outcomes {
            let verdict = if row.matched { "match" } else { "mismatch" };
            println!(
                "  {}: {} expected={} actual={} exit={:?}",
                row.name, verdict, row.expected_status, row.actual_status, row.actual_exit_code
            );
        }
        print_attachment_result(&result);
    }

    Ok(())
}

struct AttestationRequest {
    revision: String,
    capsule: Option<String>,
    file: PathBuf,
    agent: String,
    subject_name: Option<String>,
    subject_digest: Option<String>,
    builder_id: Option<String>,
    build_type: Option<String>,
    dry_run: bool,
    json: bool,
}

fn run_attach_attestation(request: AttestationRequest) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let (revision_id, revision) = load_revision(&store, &request.revision)?;
    let (capsule_id, capsule) = match request.capsule.as_deref() {
        Some(value) => load_capsule(&store, value)?,
        None => load_default_capsule(&store, &revision_id, &revision)?,
    };
    if capsule.revision_id != revision_id {
        anyhow::bail!(
            "capsule {} is bound to revision {}, not {}",
            capsule_id.to_hex(),
            capsule.revision_id.to_hex(),
            revision_id.to_hex()
        );
    }

    let bytes = std::fs::read(&request.file)?;
    let statement: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| anyhow::anyhow!("attestation must be JSON: {err}"))?;
    let attestation = attestation_evidence_result(
        &request.file,
        &bytes,
        &statement,
        request.subject_name.as_deref(),
        request.subject_digest.as_deref(),
        request.builder_id.as_deref(),
        request.build_type.as_deref(),
        &revision_id,
    )?;
    let evidence = attestation.evidence;
    let status = evidence.status.clone();
    let summary = evidence.summary.clone().unwrap_or_default();
    let result = attach_evidence(
        &store,
        &capsule_id,
        capsule,
        &request.agent,
        vec![evidence],
        request.dry_run,
    )?;

    if request.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": PROVENANCE_JSON_SCHEMA_VERSION,
                "action": "provenance.attach_attestation",
                "attestation": {
                    "file": request.file,
                    "status": status,
                    "summary": summary,
                    "statement_type": attestation.report["statement_type"].clone(),
                    "predicate_type": attestation.report["predicate_type"].clone(),
                    "subject_count": attestation.report["subject_count"].clone(),
                    "builder_id": attestation.report["builder_id"].clone(),
                    "build_type": attestation.report["build_type"].clone(),
                    "validation": attestation.report["validation"].clone(),
                },
                "attachment": result,
            }))?
        );
    } else {
        println!("Attached attestation evidence: {status}");
        if !summary.is_empty() {
            println!("  {summary}");
        }
        print_attachment_result(&result);
    }

    Ok(())
}

fn attach_evidence(
    store: &ClawStore,
    source_capsule_id: &claw_core::id::ObjectId,
    mut capsule: Capsule,
    agent: &str,
    evidence: Vec<Evidence>,
    dry_run: bool,
) -> anyhow::Result<AttachmentResult> {
    let evidence_added = evidence.len();
    capsule.public_fields.evidence.extend(evidence);

    if dry_run {
        return Ok(AttachmentResult {
            dry_run,
            source_capsule: source_capsule_id.to_hex(),
            updated_capsule: None,
            revision: capsule.revision_id.to_hex(),
            evidence_added,
        });
    }

    let registered_agent = ensure_registered_signing_agent(store, agent)?;
    let keypair = keypair_for_agent(agent, &registered_agent)?;
    capsule.public_fields.agent_id = registered_agent.agent_id.clone();
    capsule.public_fields.agent_version = registered_agent.agent_version.clone();
    capsule.key_id = Some(registered_agent.public_key.clone());
    capsule.signatures.clear();
    append_capsule_signature(&mut capsule, &keypair)?;

    let updated_capsule_id = store.store_object(&Object::Capsule(capsule.clone()))?;
    store.set_ref(
        &format!("capsules/{}", capsule.revision_id.to_hex()),
        &updated_capsule_id,
    )?;
    store.set_ref(
        &format!("capsules/by-revision/{}", capsule.revision_id.to_hex()),
        &updated_capsule_id,
    )?;
    store.set_ref(
        &format!(
            "capsules/by-revision/{}",
            &capsule.revision_id.to_hex()[..16]
        ),
        &updated_capsule_id,
    )?;
    store.set_ref(
        &format!(
            "capsules/history/{}/{}",
            capsule.revision_id.to_hex(),
            updated_capsule_id.to_hex()
        ),
        &updated_capsule_id,
    )?;

    Ok(AttachmentResult {
        dry_run,
        source_capsule: source_capsule_id.to_hex(),
        updated_capsule: Some(updated_capsule_id.to_hex()),
        revision: capsule.revision_id.to_hex(),
        evidence_added,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayWorkspaceKind {
    Sandbox,
    InPlace,
}

impl ReplayWorkspaceKind {
    fn as_str(self) -> &'static str {
        match self {
            ReplayWorkspaceKind::Sandbox => "sandbox",
            ReplayWorkspaceKind::InPlace => "repository",
        }
    }
}

struct ReplayWorkspace {
    path: PathBuf,
    kind: ReplayWorkspaceKind,
    _tempdir: Option<tempfile::TempDir>,
}

impl ReplayWorkspace {
    fn prepare(root: &Path, in_place: bool) -> anyhow::Result<Self> {
        if in_place {
            return Ok(Self {
                path: root.to_path_buf(),
                kind: ReplayWorkspaceKind::InPlace,
                _tempdir: None,
            });
        }

        let tempdir = tempfile::Builder::new()
            .prefix("claw-provenance-replay-")
            .tempdir()?;
        let sandbox_root = tempdir.path().join("worktree");
        copy_worktree(root, &sandbox_root)?;
        Ok(Self {
            path: sandbox_root,
            kind: ReplayWorkspaceKind::Sandbox,
            _tempdir: Some(tempdir),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn kind(&self) -> ReplayWorkspaceKind {
        self.kind
    }
}

fn replay_one(
    root: &Path,
    workspace: ReplayWorkspaceKind,
    evidence: &Evidence,
    timeout_ms: u64,
) -> anyhow::Result<ReplayOutcome> {
    let command = evidence.command.as_deref().unwrap_or_default();
    let started = current_time_ms();
    let timer = Instant::now();
    let mut child = shell_command(command)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| anyhow::anyhow!("failed to replay '{}': {err}", evidence.name))?;

    let timeout = Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if timer.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let ended = current_time_ms();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }

    let actual_exit_code = status.code();
    let actual_status = if !timed_out && actual_exit_code == Some(0) {
        "pass"
    } else {
        "fail"
    }
    .to_string();
    let status_matched = evidence
        .exit_code
        .map(|expected| Some(expected) == actual_exit_code)
        .unwrap_or_else(|| evidence.status.eq_ignore_ascii_case(&actual_status));
    let mut log = stdout.clone();
    log.extend_from_slice(&stderr);
    let actual_log_digest = format!("sha256:{}", sha256_hex(&log));
    let log_digest_matched = evidence
        .log_digest
        .as_ref()
        .map(|expected| normalize_digest(expected) == normalize_digest(&actual_log_digest));
    let matched = status_matched && log_digest_matched.unwrap_or(true);

    Ok(ReplayOutcome {
        row: ReplayRow {
            name: evidence.name.clone(),
            command: command.to_string(),
            expected_status: evidence.status.clone(),
            expected_exit_code: evidence.exit_code,
            expected_log_digest: evidence.log_digest.clone(),
            actual_status,
            actual_exit_code,
            actual_log_digest,
            log_digest_matched,
            matched,
            started_at_ms: started,
            ended_at_ms: ended,
            duration_ms: ended.saturating_sub(started),
            timed_out,
            workspace: workspace.as_str().to_string(),
        },
        workspace,
    })
}

fn replay_evidence(
    capsule_id: &claw_core::id::ObjectId,
    source: &Evidence,
    outcome: &ReplayOutcome,
) -> Evidence {
    Evidence {
        name: format!("replay:{}", source.name),
        status: if outcome.row.matched { "pass" } else { "fail" }.to_string(),
        duration_ms: outcome.row.duration_ms,
        artifact_refs: vec![format!("capsule:{}", capsule_id.to_hex())],
        summary: Some(format!(
            "replayed '{}' in {} workspace and compared expected status={} exit={:?} log_digest={:?} with actual status={} exit={:?} log_digest={}",
            source.name,
            outcome.workspace.as_str(),
            outcome.row.expected_status,
            outcome.row.expected_exit_code,
            outcome.row.expected_log_digest,
            outcome.row.actual_status,
            outcome.row.actual_exit_code,
            outcome.row.actual_log_digest
        )),
        revision_id: source.revision_id,
        command: Some(outcome.row.command.clone()),
        exit_code: outcome.row.actual_exit_code,
        started_at_ms: Some(outcome.row.started_at_ms),
        ended_at_ms: Some(outcome.row.ended_at_ms),
        environment_digest: source.environment_digest.clone(),
        runner_identity: Some(format!(
            "claw provenance replay ({})",
            outcome.workspace.as_str()
        )),
        log_digest: Some(outcome.row.actual_log_digest.clone()),
        artifact_digest: None,
        expires_at_ms: None,
        trust_domain: Some("provenance-replay".to_string()),
        signature: None,
    }
}

fn copy_worktree(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    copy_worktree_dir(source, destination)
}

fn copy_worktree_dir(source: &Path, destination: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_lossy = name.to_string_lossy();
        if matches!(name_lossy.as_ref(), ".claw" | ".git") {
            continue;
        }
        let src = entry.path();
        let dst = destination.join(&name);
        let metadata = std::fs::symlink_metadata(&src)?;
        if metadata.is_dir() {
            std::fs::create_dir_all(&dst)?;
            copy_worktree_dir(&src, &dst)?;
        } else if metadata.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn attestation_evidence(
    path: &std::path::Path,
    bytes: &[u8],
    statement: &serde_json::Value,
    subject_name: Option<&str>,
    subject_digest: Option<&str>,
    revision_id: &claw_core::id::ObjectId,
) -> anyhow::Result<Evidence> {
    Ok(attestation_evidence_result(
        path,
        bytes,
        statement,
        subject_name,
        subject_digest,
        None,
        None,
        revision_id,
    )?
    .evidence)
}

struct AttestationEvidenceResult {
    evidence: Evidence,
    report: serde_json::Value,
}

#[allow(clippy::too_many_arguments)]
fn attestation_evidence_result(
    path: &std::path::Path,
    bytes: &[u8],
    statement: &serde_json::Value,
    subject_name: Option<&str>,
    subject_digest: Option<&str>,
    builder_id: Option<&str>,
    build_type: Option<&str>,
    revision_id: &claw_core::id::ObjectId,
) -> anyhow::Result<AttestationEvidenceResult> {
    let predicate_type = statement
        .get("predicateType")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let statement_type = statement
        .get("_type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let name = if predicate_type.contains("slsa.dev/provenance") {
        "slsa.provenance"
    } else if statement_type.contains("in-toto.io/Statement") || !predicate_type.is_empty() {
        "in-toto.statement"
    } else {
        anyhow::bail!("attestation is not an in-toto statement and has no predicateType");
    };

    let subject_check = check_subject(statement, subject_name, subject_digest)?;
    let predicate_check = check_predicate(statement, predicate_type, builder_id, build_type);
    let status = if subject_check.matched && predicate_check.matched {
        "pass"
    } else {
        "fail"
    };
    let mut refs = vec![path.display().to_string()];
    refs.extend(subject_check.subjects);
    if let Some(builder_id) = &predicate_check.builder_id {
        refs.push(format!("builder:{builder_id}"));
    }
    if let Some(build_type) = &predicate_check.build_type {
        refs.push(format!("buildType:{build_type}"));
    }
    let summary = format!("{}; {}", subject_check.summary, predicate_check.summary);

    let evidence = Evidence {
        name: name.to_string(),
        status: status.to_string(),
        duration_ms: 0,
        artifact_refs: refs,
        summary: Some(summary),
        revision_id: Some(*revision_id),
        command: None,
        exit_code: Some(if status == "pass" { 0 } else { 1 }),
        started_at_ms: Some(current_time_ms()),
        ended_at_ms: Some(current_time_ms()),
        environment_digest: None,
        runner_identity: Some("claw provenance attestation".to_string()),
        log_digest: None,
        artifact_digest: Some(format!("sha256:{}", sha256_hex(bytes))),
        expires_at_ms: None,
        trust_domain: Some("supply-chain-attestation".to_string()),
        signature: None,
    };
    let report = serde_json::json!({
        "statement_type": statement_type,
        "predicate_type": predicate_type,
        "subject_count": subject_check.subject_count,
        "builder_id": predicate_check.builder_id,
        "build_type": predicate_check.build_type,
        "validation": {
            "subject_matched": subject_check.matched,
            "predicate_matched": predicate_check.matched,
            "expected_subject_name": subject_name,
            "expected_subject_digest": subject_digest,
            "expected_builder_id": builder_id,
            "expected_build_type": build_type,
            "errors": predicate_check.errors,
        }
    });

    Ok(AttestationEvidenceResult { evidence, report })
}

struct SubjectCheck {
    matched: bool,
    summary: String,
    subjects: Vec<String>,
    subject_count: usize,
}

struct PredicateCheck {
    matched: bool,
    summary: String,
    builder_id: Option<String>,
    build_type: Option<String>,
    errors: Vec<String>,
}

struct ExpectedDigest {
    algorithm: Option<String>,
    value: String,
}

fn check_subject(
    statement: &serde_json::Value,
    subject_name: Option<&str>,
    subject_digest: Option<&str>,
) -> anyhow::Result<SubjectCheck> {
    let subjects = statement
        .get("subject")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let expected_digest = subject_digest.map(parse_expected_digest);
    if subjects.is_empty() {
        return Ok(SubjectCheck {
            matched: subject_name.is_none() && subject_digest.is_none(),
            summary: "attestation contains no subjects".to_string(),
            subjects: vec![],
            subject_count: 0,
        });
    }

    let mut rendered = Vec::new();
    let mut matched = subject_name.is_none() && subject_digest.is_none();
    for subject in subjects {
        let name = subject
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let digest = subject.get("digest").and_then(|value| value.as_object());
        let digest_match = match (&expected_digest, digest) {
            (None, _) => true,
            (Some(expected), Some(digest)) => subject_digest_matches(expected, digest),
            (Some(_), None) => false,
        };
        let name_match = subject_name.is_none_or(|expected| expected == name);
        if name_match && digest_match {
            matched = true;
        }
        rendered.push(format!(
            "subject:{}",
            if name.is_empty() { "<unnamed>" } else { name }
        ));
    }

    let summary = if matched {
        "attestation subject matched requested constraints".to_string()
    } else {
        "attestation subject did not match requested constraints".to_string()
    };
    let subject_count = rendered.len();
    Ok(SubjectCheck {
        matched,
        summary,
        subjects: rendered,
        subject_count,
    })
}

fn check_predicate(
    statement: &serde_json::Value,
    predicate_type: &str,
    expected_builder_id: Option<&str>,
    expected_build_type: Option<&str>,
) -> PredicateCheck {
    let predicate = statement.get("predicate");
    let builder_id = predicate
        .and_then(|predicate| predicate.get("builder"))
        .and_then(|builder| builder.get("id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let build_type = predicate
        .and_then(|predicate| predicate.get("buildType"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let is_slsa = predicate_type.contains("slsa.dev/provenance");
    let mut errors = Vec::new();

    if is_slsa && predicate.is_none() {
        errors.push("SLSA provenance missing predicate".to_string());
    }
    if is_slsa && builder_id.as_deref().unwrap_or("").trim().is_empty() {
        errors.push("SLSA provenance missing predicate.builder.id".to_string());
    }
    if is_slsa && build_type.as_deref().unwrap_or("").trim().is_empty() {
        errors.push("SLSA provenance missing predicate.buildType".to_string());
    }
    if let Some(expected) = expected_builder_id {
        if builder_id.as_deref() != Some(expected) {
            errors.push(format!(
                "builder.id did not match expected value '{expected}'"
            ));
        }
    }
    if let Some(expected) = expected_build_type {
        if build_type.as_deref() != Some(expected) {
            errors.push(format!(
                "buildType did not match expected value '{expected}'"
            ));
        }
    }

    let summary = if errors.is_empty() {
        if is_slsa {
            "SLSA predicate metadata matched requested constraints".to_string()
        } else if expected_builder_id.is_some() || expected_build_type.is_some() {
            "in-toto predicate metadata matched requested constraints".to_string()
        } else {
            "no predicate constraints requested".to_string()
        }
    } else {
        format!("predicate validation failed: {}", errors.join("; "))
    };

    PredicateCheck {
        matched: errors.is_empty(),
        summary,
        builder_id,
        build_type,
        errors,
    }
}

fn parse_expected_digest(value: &str) -> ExpectedDigest {
    let trimmed = value.trim();
    if let Some((algorithm, digest)) = trimmed.split_once(':') {
        if !algorithm.trim().is_empty() && !digest.trim().is_empty() {
            return ExpectedDigest {
                algorithm: Some(algorithm.trim().to_ascii_lowercase()),
                value: digest.trim().to_ascii_lowercase(),
            };
        }
    }
    ExpectedDigest {
        algorithm: None,
        value: normalize_digest(trimmed),
    }
}

fn subject_digest_matches(
    expected: &ExpectedDigest,
    digest: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    match expected.algorithm.as_deref() {
        Some(algorithm) => digest.iter().any(|(actual_algorithm, value)| {
            actual_algorithm.eq_ignore_ascii_case(algorithm)
                && value
                    .as_str()
                    .map(normalize_digest)
                    .is_some_and(|actual| actual == expected.value)
        }),
        None => digest.values().any(|value| {
            value
                .as_str()
                .map(normalize_digest)
                .is_some_and(|actual| actual == expected.value)
        }),
    }
}

fn normalize_digest(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase()
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn print_attachment_result(result: &AttachmentResult) {
    if result.dry_run {
        println!(
            "  Dry run: would add {} evidence item(s) to capsule {}",
            result.evidence_added, result.source_capsule
        );
    } else if let Some(updated) = &result.updated_capsule {
        println!("  Updated capsule: {updated}");
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::json;

    use super::{
        attestation_evidence, attestation_evidence_result, replay_evidence, replay_one, sha256_hex,
        ProvenanceArgs, ProvenanceCommand, ReplayWorkspaceKind,
    };

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: ProvenanceArgs,
    }

    #[test]
    fn parses_replay_filters() {
        let cli = TestCli::parse_from([
            "claw",
            "--json",
            "replay",
            "--revision",
            "heads/main",
            "--evidence",
            "test",
            "--dry-run",
        ]);
        assert!(cli.args.json);
        match cli.args.command {
            ProvenanceCommand::Replay {
                revision,
                evidence_names,
                in_place,
                dry_run,
                ..
            } => {
                assert_eq!(revision, "heads/main");
                assert_eq!(evidence_names, vec!["test"]);
                assert!(!in_place);
                assert!(dry_run);
            }
            _ => panic!("expected replay"),
        }
    }

    #[test]
    fn classifies_slsa_attestation() {
        let statement = json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://slsa.dev/provenance/v1",
            "subject": [{
                "name": "artifact.tar.gz",
                "digest": {"sha256": "abc123"}
            }],
            "predicate": {
                "builder": {"id": "https://github.com/actions/runner"},
                "buildType": "https://github.com/Actions"
            }
        });
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"r");
        let evidence = attestation_evidence(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("sha256:abc123"),
            &revision,
        )
        .unwrap();
        assert_eq!(evidence.name, "slsa.provenance");
        assert_eq!(evidence.status, "pass");
        assert_eq!(evidence.revision_id, Some(revision));
    }

    #[test]
    fn slsa_attestation_requires_builder_and_build_type() {
        let statement = json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://slsa.dev/provenance/v1",
            "subject": [{
                "name": "artifact.tar.gz",
                "digest": {"sha256": "abc123"}
            }],
            "predicate": {}
        });
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"r");
        let result = attestation_evidence_result(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("sha256:abc123"),
            None,
            None,
            &revision,
        )
        .unwrap();

        assert_eq!(result.evidence.name, "slsa.provenance");
        assert_eq!(result.evidence.status, "fail");
        assert!(result.report["validation"]["errors"]
            .as_array()
            .expect("validation errors")
            .iter()
            .any(|error| error
                .as_str()
                .unwrap_or_default()
                .contains("predicate.builder.id")));
    }

    #[test]
    fn slsa_attestation_checks_expected_builder_and_build_type() {
        let statement = json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://slsa.dev/provenance/v1",
            "subject": [{
                "name": "artifact.tar.gz",
                "digest": {"sha256": "abc123"}
            }],
            "predicate": {
                "builder": {"id": "builder-a"},
                "buildType": "release"
            }
        });
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"r");
        let matched = attestation_evidence_result(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("sha256:abc123"),
            Some("builder-a"),
            Some("release"),
            &revision,
        )
        .unwrap();
        assert_eq!(matched.evidence.status, "pass");
        assert_eq!(matched.report["builder_id"], "builder-a");
        assert_eq!(matched.report["build_type"], "release");

        let mismatched = attestation_evidence_result(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("sha256:abc123"),
            Some("builder-b"),
            Some("release"),
            &revision,
        )
        .unwrap();
        assert_eq!(mismatched.evidence.status, "fail");
        assert!(mismatched.report["validation"]["errors"]
            .as_array()
            .expect("validation errors")
            .iter()
            .any(|error| error
                .as_str()
                .unwrap_or_default()
                .contains("builder.id did not match")));
    }

    #[test]
    fn failed_subject_match_marks_attestation_failed() {
        let statement = json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://in-toto.io/attestation/test",
            "subject": [{"name": "other", "digest": {"sha256": "def456"}}],
            "predicate": {}
        });
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"r");
        let evidence = attestation_evidence(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("abc123"),
            &revision,
        )
        .unwrap();
        assert_eq!(evidence.name, "in-toto.statement");
        assert_eq!(evidence.status, "fail");
    }

    #[test]
    fn subject_digest_algorithm_must_match_when_qualified() {
        let statement = json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://in-toto.io/attestation/test",
            "subject": [{
                "name": "artifact.tar.gz",
                "digest": {"sha512": "abc123"}
            }],
            "predicate": {}
        });
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"r");
        let qualified = attestation_evidence(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("sha256:abc123"),
            &revision,
        )
        .unwrap();
        assert_eq!(qualified.status, "fail");

        let raw = attestation_evidence(
            std::path::Path::new("attestation.json"),
            br#"{}"#,
            &statement,
            Some("artifact.tar.gz"),
            Some("abc123"),
            &revision,
        )
        .unwrap();
        assert_eq!(raw.status, "pass");
    }

    #[test]
    fn replay_compares_claimed_log_digest_when_present() {
        let tempdir = tempfile::tempdir().expect("create replay temp dir");
        let digest = format!("sha256:{}", sha256_hex(b"hello\n"));
        let evidence = claw_core::types::Evidence {
            name: "test".to_string(),
            status: "pass".to_string(),
            duration_ms: 0,
            artifact_refs: vec![],
            summary: None,
            revision_id: None,
            command: Some("printf 'hello\\n'".to_string()),
            exit_code: Some(0),
            started_at_ms: None,
            ended_at_ms: None,
            environment_digest: None,
            runner_identity: None,
            log_digest: Some(digest),
            artifact_digest: None,
            expires_at_ms: None,
            trust_domain: None,
            signature: None,
        };

        let outcome = replay_one(
            tempdir.path(),
            ReplayWorkspaceKind::Sandbox,
            &evidence,
            1_000,
        )
        .expect("replay command");
        assert_eq!(outcome.row.log_digest_matched, Some(true));
        assert!(outcome.row.matched);
        assert!(outcome.row.started_at_ms <= outcome.row.ended_at_ms);
    }

    #[test]
    fn replay_fails_when_claimed_log_digest_differs() {
        let tempdir = tempfile::tempdir().expect("create replay temp dir");
        let mut evidence = claw_core::types::Evidence {
            name: "test".to_string(),
            status: "pass".to_string(),
            duration_ms: 0,
            artifact_refs: vec![],
            summary: None,
            revision_id: None,
            command: Some("printf 'hello\\n'".to_string()),
            exit_code: Some(0),
            started_at_ms: None,
            ended_at_ms: None,
            environment_digest: None,
            runner_identity: None,
            log_digest: Some(format!("sha256:{}", sha256_hex(b"different\n"))),
            artifact_digest: None,
            expires_at_ms: None,
            trust_domain: None,
            signature: None,
        };

        let outcome = replay_one(
            tempdir.path(),
            ReplayWorkspaceKind::Sandbox,
            &evidence,
            1_000,
        )
        .expect("replay command");
        assert_eq!(outcome.row.log_digest_matched, Some(false));
        assert!(!outcome.row.matched);

        evidence.log_digest = None;
        let outcome_without_log_claim = replay_one(
            tempdir.path(),
            ReplayWorkspaceKind::Sandbox,
            &evidence,
            1_000,
        )
        .expect("replay command");
        assert_eq!(outcome_without_log_claim.row.log_digest_matched, None);
        assert!(outcome_without_log_claim.row.matched);
    }

    #[test]
    fn replay_evidence_preserves_replay_timestamps() {
        let tempdir = tempfile::tempdir().expect("create replay temp dir");
        let revision = claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"r");
        let capsule = claw_core::hash::content_hash(claw_core::object::TypeTag::Capsule, b"c");
        let evidence = claw_core::types::Evidence {
            name: "test".to_string(),
            status: "pass".to_string(),
            duration_ms: 0,
            artifact_refs: vec![],
            summary: None,
            revision_id: Some(revision),
            command: Some("printf 'hello\\n'".to_string()),
            exit_code: Some(0),
            started_at_ms: None,
            ended_at_ms: None,
            environment_digest: Some("sha256:env".to_string()),
            runner_identity: None,
            log_digest: None,
            artifact_digest: None,
            expires_at_ms: None,
            trust_domain: None,
            signature: None,
        };

        let outcome = replay_one(
            tempdir.path(),
            ReplayWorkspaceKind::Sandbox,
            &evidence,
            1_000,
        )
        .expect("replay command");
        let replay = replay_evidence(&capsule, &evidence, &outcome);

        assert_eq!(replay.name, "replay:test");
        assert_eq!(replay.status, "pass");
        assert_eq!(replay.revision_id, Some(revision));
        assert_eq!(replay.started_at_ms, Some(outcome.row.started_at_ms));
        assert_eq!(replay.ended_at_ms, Some(outcome.row.ended_at_ms));
        assert_eq!(replay.environment_digest.as_deref(), Some("sha256:env"));
        assert_eq!(replay.trust_domain.as_deref(), Some("provenance-replay"));
    }
}
