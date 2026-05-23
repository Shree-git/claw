use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;

use claw_core::hash::content_hash;
use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::{Capsule, CapsulePublic, EvidencePolicy, Policy, Revision, Visibility};
use claw_policy::checks::{
    verify_authorized_recipients, verify_evidence_freshness, verify_min_trust_score,
    verify_quarantine_lane, verify_required_checks, verify_required_reviewers,
    verify_sensitive_paths,
};
use claw_policy::plugin::evaluate_plugins;
use claw_policy::visibility::check_visibility;
use claw_policy::PolicyContext;
use claw_store::tree_diff::diff_trees;
use claw_store::ClawStore;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct PolicyArgs {
    #[command(subcommand)]
    command: PolicyCommand,
}

#[derive(Subcommand)]
enum PolicyCommand {
    /// Create or update a policy
    Create {
        /// Policy ID
        #[arg(long)]
        id: String,
        /// Visibility: public|private|encrypted-metadata-required
        #[arg(long, default_value = "public")]
        visibility: String,
        /// Required check (repeat for multiple checks)
        #[arg(long = "check")]
        checks: Vec<String>,
        /// Required reviewer identity (repeatable)
        #[arg(long = "reviewer")]
        reviewers: Vec<String>,
        /// Sensitive path glob (repeatable)
        #[arg(long = "sensitive-path")]
        sensitive_paths: Vec<String>,
        /// Mark policy as quarantine lane
        #[arg(long)]
        quarantine_lane: bool,
        /// Optional minimum trust score threshold (e.g. 0.8 or 80%)
        #[arg(long)]
        min_trust_score: Option<String>,
        /// Authorized recipient ID for encrypted private fields (repeatable)
        #[arg(long = "recipient")]
        recipients: Vec<String>,
        /// Revoked recipient ID that must not appear in capsule envelopes (repeatable)
        #[arg(long = "revoked-recipient")]
        revoked_recipients: Vec<String>,
        /// Require evidence freshness metadata for required checks
        #[arg(long)]
        require_fresh_evidence: bool,
        /// Maximum evidence age in milliseconds when freshness is required
        #[arg(long)]
        evidence_max_age_ms: Option<u64>,
        /// Trusted runner identity for fresh evidence (repeatable)
        #[arg(long = "trusted-runner")]
        trusted_runners: Vec<String>,
    },
    /// Preview or apply a policy definition
    Apply {
        /// Policy ID
        #[arg(long)]
        id: String,
        /// Visibility: public|private|encrypted-metadata-required
        #[arg(long, default_value = "public")]
        visibility: String,
        /// Required check (repeat for multiple checks)
        #[arg(long = "check")]
        checks: Vec<String>,
        /// Required reviewer identity (repeatable)
        #[arg(long = "reviewer")]
        reviewers: Vec<String>,
        /// Sensitive path glob (repeatable)
        #[arg(long = "sensitive-path")]
        sensitive_paths: Vec<String>,
        /// Mark policy as quarantine lane
        #[arg(long)]
        quarantine_lane: bool,
        /// Optional minimum trust score threshold (e.g. 0.8 or 80%)
        #[arg(long)]
        min_trust_score: Option<String>,
        /// Authorized recipient ID for encrypted private fields (repeatable)
        #[arg(long = "recipient")]
        recipients: Vec<String>,
        /// Revoked recipient ID that must not appear in capsule envelopes (repeatable)
        #[arg(long = "revoked-recipient")]
        revoked_recipients: Vec<String>,
        /// Require evidence freshness metadata for required checks
        #[arg(long)]
        require_fresh_evidence: bool,
        /// Maximum evidence age in milliseconds when freshness is required
        #[arg(long)]
        evidence_max_age_ms: Option<u64>,
        /// Trusted runner identity for fresh evidence (repeatable)
        #[arg(long = "trusted-runner")]
        trusted_runners: Vec<String>,
        /// Preview object/ref changes without writing them
        #[arg(long)]
        dry_run: bool,
        /// Output apply result as JSON
        #[arg(long)]
        json: bool,
    },
    /// Evaluate or simulate a policy against a revision and capsule
    #[command(alias = "simulate")]
    Eval {
        /// Policy ID or policy ref
        #[arg(required_unless_present = "policy_file")]
        id: Option<String>,
        /// Evaluate an arbitrary policy JSON/TOML file without storing it.
        #[arg(long = "policy-file")]
        policy_file: Option<PathBuf>,
        /// Revision ref, hex ID, or clw_ display ID
        #[arg(long)]
        revision: String,
        /// Capsule ref, hex ID, or clw_ display ID. Defaults to the revision capsule.
        #[arg(long)]
        capsule: Option<String>,
        /// Verified signer agent ID (repeatable)
        #[arg(long = "signer-agent")]
        signer_agents: Vec<String>,
        /// Verified signer key ID (repeatable)
        #[arg(long = "signer-key")]
        signer_keys: Vec<String>,
        /// Touched path for sensitive-path evaluation (repeatable)
        #[arg(long = "path")]
        touched_paths: Vec<String>,
        /// Trust score override (e.g. 0.8 or 80%). Defaults to capsule evidence pass ratio.
        #[arg(long)]
        trust_score: Option<String>,
        /// Output evaluation result as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a policy
    Show {
        /// Policy ID
        id: String,
    },
    /// Lint policies for dangerous or surprising enforcement gaps
    Lint {
        /// Policy ID. Omit to lint all stored policies.
        id: Option<String>,
        /// Output lint report as JSON
        #[arg(long)]
        json: bool,
    },
    /// List policies
    List,
}

pub fn run(args: PolicyArgs) -> anyhow::Result<()> {
    match args.command {
        PolicyCommand::Create {
            id,
            visibility,
            checks,
            reviewers,
            sensitive_paths,
            quarantine_lane,
            min_trust_score,
            recipients,
            revoked_recipients,
            require_fresh_evidence,
            evidence_max_age_ms,
            trusted_runners,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let policy = build_policy(PolicyBuildOptions {
                id,
                visibility,
                checks,
                reviewers,
                sensitive_paths,
                quarantine_lane,
                min_trust_score,
                recipients,
                revoked_recipients,
                require_fresh_evidence,
                evidence_max_age_ms,
                trusted_runners,
            })?;
            save_policy(&store, &policy, false, false)?;
        }
        PolicyCommand::Apply {
            id,
            visibility,
            checks,
            reviewers,
            sensitive_paths,
            quarantine_lane,
            min_trust_score,
            recipients,
            revoked_recipients,
            require_fresh_evidence,
            evidence_max_age_ms,
            trusted_runners,
            dry_run,
            json,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let policy = build_policy(PolicyBuildOptions {
                id,
                visibility,
                checks,
                reviewers,
                sensitive_paths,
                quarantine_lane,
                min_trust_score,
                recipients,
                revoked_recipients,
                require_fresh_evidence,
                evidence_max_age_ms,
                trusted_runners,
            })?;
            save_policy(&store, &policy, dry_run, json)?;
        }
        PolicyCommand::Eval {
            id,
            policy_file,
            revision,
            capsule,
            signer_agents,
            signer_keys,
            touched_paths,
            trust_score,
            json,
        } => {
            run_eval(EvalRequest {
                policy_id: id,
                policy_file,
                revision_ref: revision,
                capsule_ref: capsule,
                signer_agents,
                signer_keys,
                touched_paths,
                trust_score,
                json,
            })?;
        }
        PolicyCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let ref_name = if id.starts_with("policies/") {
                id
            } else {
                format!("policies/{id}")
            };
            let obj_id = store
                .get_ref(&ref_name)?
                .ok_or_else(|| anyhow::anyhow!("policy not found: {ref_name}"))?;
            let obj = store.load_object(&obj_id)?;

            let policy = match obj {
                Object::Policy(p) => p,
                _ => anyhow::bail!("ref does not point to a policy object: {ref_name}"),
            };

            println!("Policy: {}", policy.policy_id);
            println!("  Ref: {ref_name}");
            println!("  Visibility: {:?}", policy.visibility);
            if !policy.required_checks.is_empty() {
                println!("  Required checks: {}", policy.required_checks.join(", "));
            }
            if !policy.required_reviewers.is_empty() {
                println!(
                    "  Required reviewers: {}",
                    policy.required_reviewers.join(", ")
                );
            }
            if !policy.sensitive_paths.is_empty() {
                println!("  Sensitive paths: {}", policy.sensitive_paths.join(", "));
            }
            if policy.quarantine_lane {
                println!("  Quarantine lane: true");
            }
            if let Some(score) = policy.min_trust_score {
                println!("  Min trust score: {score}");
            }
            if !policy.authorized_recipients.is_empty() {
                println!(
                    "  Authorized recipients: {}",
                    policy.authorized_recipients.join(", ")
                );
            }
            if !policy.revoked_recipients.is_empty() {
                println!(
                    "  Revoked recipients: {}",
                    policy.revoked_recipients.join(", ")
                );
            }
            if policy.evidence_policy.require_fresh_evidence {
                println!("  Fresh evidence: required");
                if let Some(max_age_ms) = policy.evidence_policy.max_age_ms {
                    println!("  Evidence max age ms: {max_age_ms}");
                }
                if !policy.evidence_policy.trusted_runner_identities.is_empty() {
                    println!(
                        "  Trusted runners: {}",
                        policy.evidence_policy.trusted_runner_identities.join(", ")
                    );
                }
            }
        }
        PolicyCommand::Lint { id, json } => {
            run_lint(id.as_deref(), json)?;
        }
        PolicyCommand::List => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let refs = store.list_refs("policies")?;

            if refs.is_empty() {
                println!("No policies found.");
                return Ok(());
            }

            for (name, obj_id) in refs {
                match store.load_object(&obj_id) {
                    Ok(Object::Policy(policy)) => {
                        println!(
                            "{} {:?} checks:{}",
                            policy.policy_id,
                            policy.visibility,
                            policy.required_checks.len()
                        );
                    }
                    _ => {
                        println!("{} (non-policy object)", name);
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct PolicyLintReport {
    schema_version: u8,
    action: &'static str,
    policy_count: usize,
    finding_count: usize,
    danger_count: usize,
    warning_count: usize,
    policies: Vec<PolicyLintSubject>,
}

#[derive(Debug, Clone, Serialize)]
struct PolicyLintSubject {
    id: String,
    ref_name: String,
    object: String,
    findings: Vec<PolicyLintFinding>,
}

#[derive(Debug, Clone, Serialize)]
struct PolicyLintFinding {
    severity: &'static str,
    code: &'static str,
    message: String,
    why: String,
    try_command: Option<String>,
}

fn run_lint(id: Option<&str>, json: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let subjects = if let Some(id) = id {
        let (ref_name, object, policy) = load_policy(&store, id)?;
        vec![lint_subject(ref_name, object, policy)]
    } else {
        let mut subjects = Vec::new();
        for (ref_name, object) in store.list_refs("policies")? {
            if let Ok(Object::Policy(policy)) = store.load_object(&object) {
                subjects.push(lint_subject(ref_name, object, policy));
            }
        }
        subjects
    };
    let report = lint_report(subjects);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_lint_report(&report);
    }

    Ok(())
}

fn lint_subject(ref_name: String, object: ObjectId, policy: Policy) -> PolicyLintSubject {
    PolicyLintSubject {
        id: policy.policy_id.clone(),
        ref_name,
        object: object.to_string(),
        findings: lint_policy(&policy),
    }
}

fn lint_report(policies: Vec<PolicyLintSubject>) -> PolicyLintReport {
    let finding_count = policies
        .iter()
        .map(|subject| subject.findings.len())
        .sum::<usize>();
    let danger_count = policies
        .iter()
        .flat_map(|subject| subject.findings.iter())
        .filter(|finding| finding.severity == "danger")
        .count();
    let warning_count = policies
        .iter()
        .flat_map(|subject| subject.findings.iter())
        .filter(|finding| finding.severity == "warning")
        .count();
    PolicyLintReport {
        schema_version: 1,
        action: "policy.lint",
        policy_count: policies.len(),
        finding_count,
        danger_count,
        warning_count,
        policies,
    }
}

fn print_lint_report(report: &PolicyLintReport) {
    if report.policies.is_empty() {
        println!("No policies found.");
        return;
    }

    for subject in &report.policies {
        println!(
            "Policy {}: {} finding(s)",
            subject.id,
            subject.findings.len()
        );
        if subject.findings.is_empty() {
            println!("  ok: no lint findings");
            continue;
        }
        for finding in &subject.findings {
            println!(
                "  {} {}: {}",
                finding.severity.to_ascii_uppercase(),
                finding.code,
                finding.message
            );
            println!("    why: {}", finding.why);
            if let Some(command) = &finding.try_command {
                println!("    try: {command}");
            }
        }
    }
}

fn lint_policy(policy: &Policy) -> Vec<PolicyLintFinding> {
    let mut findings = Vec::new();
    let enforces_checks = !policy.required_checks.is_empty();
    let enforces_reviewers = !policy.required_reviewers.is_empty();
    let enforces_private_visibility = matches!(
        policy.visibility,
        Visibility::Private | Visibility::EncryptedMetadataRequired
    );
    let enforces_recipients = !policy.authorized_recipients.is_empty();
    let enforces_trust = policy.min_trust_score.is_some();
    let enforces_quarantine = policy.quarantine_lane;

    if !enforces_checks
        && !enforces_reviewers
        && !enforces_private_visibility
        && !enforces_recipients
        && !enforces_trust
        && !enforces_quarantine
    {
        findings.push(finding(
            "danger",
            "POLICY_NO_ENFORCEMENT",
            "policy does not enforce checks, reviewers, trust, private metadata, recipients, or quarantine",
            "A policy with only an ID and public visibility will allow almost everything, which is rarely useful outside demos.",
            Some(format!(
                "claw policy apply --id {} --check test --dry-run",
                policy.policy_id
            )),
        ));
    }

    if !policy.sensitive_paths.is_empty() && !enforces_private_visibility && !enforces_recipients {
        findings.push(finding(
            "danger",
            "SENSITIVE_PATHS_WITH_PUBLIC_VISIBILITY",
            "sensitive paths are configured without private metadata or recipient enforcement",
            "Sensitive path policies should make the capsule privacy requirement obvious; otherwise sensitive changes can pass with public-only capsule metadata.",
            Some(format!(
                "claw policy apply --id {} --visibility encrypted-metadata-required --sensitive-path <glob> --dry-run",
                policy.policy_id
            )),
        ));
    }

    if policy.evidence_policy.require_fresh_evidence && policy.required_checks.is_empty() {
        findings.push(finding(
            "warning",
            "FRESHNESS_WITHOUT_REQUIRED_CHECKS",
            "fresh-evidence checks apply to any capsule evidence because no required checks are named",
            "This can be intentional, but most release policies should name the exact checks that must be fresh and passing.",
            Some(format!(
                "claw policy apply --id {} --check test --require-fresh-evidence --dry-run",
                policy.policy_id
            )),
        ));
    }

    if policy.evidence_policy.require_fresh_evidence
        && policy.evidence_policy.trusted_runner_identities.is_empty()
    {
        findings.push(finding(
            "warning",
            "FRESHNESS_WITHOUT_TRUSTED_RUNNER",
            "fresh evidence does not restrict runner identities",
            "Evidence can carry runner metadata, but without trusted runners the policy does not distinguish CI-produced evidence from weaker local evidence.",
            Some(format!(
                "claw policy apply --id {} --trusted-runner github-actions/release --require-fresh-evidence --dry-run",
                policy.policy_id
            )),
        ));
    }

    if policy.quarantine_lane && policy.sensitive_paths.is_empty() {
        findings.push(finding(
            "warning",
            "QUARANTINE_WITHOUT_SENSITIVE_PATHS",
            "quarantine lane is enabled without sensitive path globs",
            "The evaluator treats this as a broad automated-integration block, which may surprise operators expecting path-scoped quarantine.",
            Some(format!(
                "claw policy apply --id {} --sensitive-path secrets/** --quarantine-lane --dry-run",
                policy.policy_id
            )),
        ));
    }

    if let Some(score) = policy.min_trust_score.as_deref() {
        if parse_trust_score(score).is_err() {
            findings.push(finding(
                "danger",
                "INVALID_TRUST_SCORE",
                "minimum trust score cannot be parsed",
                "Policy evaluation fails closed when the threshold is malformed.",
                Some(format!(
                    "claw policy apply --id {} --min-trust-score 80% --dry-run",
                    policy.policy_id
                )),
            ));
        }
    } else if enforces_checks || enforces_reviewers {
        findings.push(finding(
            "warning",
            "NO_TRUST_SCORE_THRESHOLD",
            "policy has checks or reviewers but no minimum trust score",
            "A trust threshold is optional, but adding one makes weak or partial evidence easier to reject consistently.",
            Some(format!(
                "claw policy apply --id {} --min-trust-score 80% --dry-run",
                policy.policy_id
            )),
        ));
    }

    let overlap =
        intersection_case_insensitive(&policy.authorized_recipients, &policy.revoked_recipients);
    if !overlap.is_empty() {
        findings.push(finding(
            "danger",
            "RECIPIENT_AUTH_REVOKE_OVERLAP",
            format!(
                "recipient(s) appear in both authorized and revoked lists: {}",
                overlap.join(", ")
            ),
            "A capsule cannot satisfy a policy that both requires and forbids the same recipient envelope.",
            Some(format!(
                "claw policy apply --id {} --recipient <active-recipient> --dry-run",
                policy.policy_id
            )),
        ));
    }

    for (field, values) in [
        ("required_checks", &policy.required_checks),
        ("required_reviewers", &policy.required_reviewers),
        ("sensitive_paths", &policy.sensitive_paths),
        ("authorized_recipients", &policy.authorized_recipients),
        ("revoked_recipients", &policy.revoked_recipients),
    ] {
        let duplicates = duplicate_values(values);
        if !duplicates.is_empty() {
            findings.push(finding(
                "warning",
                "DUPLICATE_POLICY_VALUES",
                format!(
                    "{} contains duplicate value(s): {}",
                    field,
                    duplicates.join(", ")
                ),
                "Duplicate values do not strengthen enforcement and make policy review noisier.",
                None,
            ));
        }
    }

    findings
}

fn finding(
    severity: &'static str,
    code: &'static str,
    message: impl Into<String>,
    why: impl Into<String>,
    try_command: Option<String>,
) -> PolicyLintFinding {
    PolicyLintFinding {
        severity,
        code,
        message: message.into(),
        why: why.into(),
        try_command,
    }
}

fn duplicate_values(values: &[String]) -> Vec<String> {
    let mut counts = HashMap::<String, usize>::new();
    for value in values {
        *counts.entry(value.to_ascii_lowercase()).or_default() += 1;
    }
    let mut duplicates = values
        .iter()
        .filter(|value| {
            counts
                .get(&value.to_ascii_lowercase())
                .copied()
                .unwrap_or(0)
                > 1
        })
        .cloned()
        .collect::<Vec<_>>();
    duplicates.sort();
    duplicates.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    duplicates
}

fn intersection_case_insensitive(left: &[String], right: &[String]) -> Vec<String> {
    let right = right
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut overlap = left
        .iter()
        .filter(|value| right.contains(&value.to_ascii_lowercase()))
        .cloned()
        .collect::<Vec<_>>();
    overlap.sort();
    overlap.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    overlap
}

struct PolicyBuildOptions {
    id: String,
    visibility: String,
    checks: Vec<String>,
    reviewers: Vec<String>,
    sensitive_paths: Vec<String>,
    quarantine_lane: bool,
    min_trust_score: Option<String>,
    recipients: Vec<String>,
    revoked_recipients: Vec<String>,
    require_fresh_evidence: bool,
    evidence_max_age_ms: Option<u64>,
    trusted_runners: Vec<String>,
}

fn build_policy(options: PolicyBuildOptions) -> anyhow::Result<Policy> {
    let visibility = parse_visibility(&options.visibility)?;
    if let Some(score) = options.min_trust_score.as_deref() {
        validate_min_trust_score(score)?;
    }

    let default_evidence_policy = EvidencePolicy::default();
    let evidence_policy = EvidencePolicy {
        require_fresh_evidence: options.require_fresh_evidence,
        max_age_ms: options
            .evidence_max_age_ms
            .or(default_evidence_policy.max_age_ms),
        trusted_runner_identities: options.trusted_runners,
        ..default_evidence_policy
    };

    Ok(Policy {
        policy_id: options.id,
        required_checks: options.checks,
        required_reviewers: options.reviewers,
        sensitive_paths: options.sensitive_paths,
        quarantine_lane: options.quarantine_lane,
        min_trust_score: options.min_trust_score,
        visibility,
        authorized_recipients: options.recipients,
        revoked_recipients: options.revoked_recipients,
        evidence_policy,
    })
}

fn save_policy(
    store: &ClawStore,
    policy: &Policy,
    dry_run: bool,
    json: bool,
) -> anyhow::Result<()> {
    let ref_name = format!("policies/{}", policy.policy_id);
    let old = store.get_ref(&ref_name)?;
    let planned_obj_id = policy_object_id(policy)?;

    if dry_run {
        if json {
            print_policy_apply_json(policy, &ref_name, old, planned_obj_id, true)?;
        } else {
            println!("Dry run: would save policy {}", policy.policy_id);
            println!("  Ref: {ref_name}");
            if let Some(old) = old {
                println!("  Previous object: {old}");
            }
            println!("  New object: {planned_obj_id}");
            println!("  Object write skipped.");
            println!("  Ref update skipped.");
        }
        return Ok(());
    }

    let obj_id = store.store_object(&Object::Policy(policy.clone()))?;
    store.update_ref_cas(
        &ref_name,
        old.as_ref(),
        &obj_id,
        "policy",
        "policy create/update",
    )?;

    if json {
        print_policy_apply_json(policy, &ref_name, old, obj_id, false)?;
    } else {
        println!("Saved policy: {}", policy.policy_id);
        println!("  Ref: {ref_name}");
        println!("  Object: {obj_id}");
    }

    Ok(())
}

fn print_policy_apply_json(
    policy: &Policy,
    ref_name: &str,
    old: Option<ObjectId>,
    new: ObjectId,
    dry_run: bool,
) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "action": "policy.apply",
            "dry_run": dry_run,
            "ref": ref_name,
            "old_object": old.map(|id| id.to_string()),
            "new_object": new.to_string(),
            "policy": policy,
        }))?
    );
    Ok(())
}

fn policy_object_id(policy: &Policy) -> anyhow::Result<ObjectId> {
    let object = Object::Policy(policy.clone());
    let payload = object.serialize_payload()?;
    Ok(content_hash(object.type_tag(), &payload))
}

struct EvalRequest {
    policy_id: Option<String>,
    policy_file: Option<PathBuf>,
    revision_ref: String,
    capsule_ref: Option<String>,
    signer_agents: Vec<String>,
    signer_keys: Vec<String>,
    touched_paths: Vec<String>,
    trust_score: Option<String>,
    json: bool,
}

#[derive(Debug, Serialize)]
struct SimulationStep {
    name: &'static str,
    description: &'static str,
    passed: bool,
    reason: Option<String>,
    inputs: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct SimulationFailure {
    step: &'static str,
    reason: String,
}

fn run_eval(request: EvalRequest) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let EvalPolicySource {
        ref_name: policy_ref,
        source: policy_source,
        object_id: policy_obj_id,
        policy,
    } = load_eval_policy(
        &store,
        request.policy_id.as_deref(),
        request.policy_file.as_deref(),
    )?;
    let (revision_id, revision) = load_revision(&store, &request.revision_ref)?;
    let (capsule_id, capsule, capsule_source) = match request.capsule_ref.as_deref() {
        Some(value) => {
            let (capsule_id, capsule) = load_capsule(&store, value)?;
            (Some(capsule_id), capsule, "explicit")
        }
        None => match load_default_capsule(&store, &revision_id, &revision)? {
            Some((capsule_id, capsule)) => (Some(capsule_id), capsule, "default"),
            None => (
                None,
                synthetic_missing_capsule(revision_id),
                "synthetic_missing",
            ),
        },
    };
    let derived_touched_paths = derive_revision_touched_paths(&store, &revision)?;
    let (touched_paths, touched_paths_source) = if request.touched_paths.is_empty() {
        (derived_touched_paths.clone(), "revision_patches")
    } else {
        (request.touched_paths, "cli")
    };
    let context = PolicyContext {
        revision_id: Some(revision_id),
        signer_agent_ids: request.signer_agents,
        signer_key_ids: request.signer_keys,
        touched_paths,
        trust_score: request
            .trust_score
            .as_deref()
            .map(parse_trust_score)
            .transpose()?
            .or_else(|| derive_capsule_trust_score(&capsule)),
        now_ms: Some(current_time_ms()),
    };

    let steps = simulate_policy_steps(&policy, &revision, &capsule, &context);
    let error = steps
        .iter()
        .find(|step| !step.passed)
        .and_then(|step| step.reason.clone());
    let allowed = error.is_none();
    let failed_steps = simulation_failures(&steps);
    let passed_step_count = steps.len().saturating_sub(failed_steps.len());
    let first_failed_step = failed_steps.first();

    if request.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "policy.eval",
                "allowed": allowed,
                "error": error,
                "policy": {
                    "id": &policy.policy_id,
                    "ref": &policy_ref,
                    "source": policy_source,
                    "object": policy_obj_id.to_string(),
                },
                "revision": {
                    "id": revision_id.to_string(),
                    "hex": revision_id.to_hex(),
                },
                "capsule": {
                    "id": capsule_id.map(|id| id.to_string()),
                    "hex": capsule_id.map(|id| id.to_hex()),
                    "source": capsule_source,
                    "synthetic": capsule_id.is_none(),
                },
                "context": {
                    "signer_agent_ids": context.signer_agent_ids,
                    "signer_key_ids": context.signer_key_ids,
                    "touched_paths": context.touched_paths,
                    "touched_paths_source": touched_paths_source,
                    "derived_touched_paths": derived_touched_paths,
                    "trust_score": context.trust_score,
                    "now_ms": context.now_ms,
                },
                "simulation": {
                    "step_count": steps.len(),
                    "passed_step_count": passed_step_count,
                    "failed_step_count": failed_steps.len(),
                    "first_failed_step": first_failed_step.map(|failure| failure.step),
                    "first_failure_reason": first_failed_step.map(|failure| failure.reason.as_str()),
                    "failed_steps": failed_steps,
                    "steps": steps,
                },
            }))?
        );
    }

    if let Some(err) = error {
        if !request.json {
            print_simulation_steps(&steps);
        }
        anyhow::bail!(
            "policy '{}' denied revision {}: {}",
            policy.policy_id,
            revision_id.to_hex(),
            err
        );
    }

    if !request.json {
        print_simulation_steps(&steps);
        println!(
            "Policy '{}' allowed revision {}",
            policy.policy_id,
            revision_id.to_hex()
        );
    }

    Ok(())
}

struct EvalPolicySource {
    ref_name: Option<String>,
    source: serde_json::Value,
    object_id: ObjectId,
    policy: Policy,
}

fn load_eval_policy(
    store: &ClawStore,
    id: Option<&str>,
    policy_file: Option<&Path>,
) -> anyhow::Result<EvalPolicySource> {
    match (id, policy_file) {
        (Some(_), Some(_)) => {
            anyhow::bail!("pass either a stored policy ID or --policy-file, not both")
        }
        (Some(id), None) => {
            let (ref_name, object_id, policy) = load_policy(store, id)?;
            Ok(EvalPolicySource {
                ref_name: Some(ref_name.clone()),
                source: serde_json::json!({
                    "kind": "stored",
                    "ref": ref_name,
                }),
                object_id,
                policy,
            })
        }
        (None, Some(path)) => {
            let policy = load_policy_file(path)?;
            let object_id = policy_object_id(&policy)?;
            Ok(EvalPolicySource {
                ref_name: None,
                source: serde_json::json!({
                    "kind": "file",
                    "path": path,
                }),
                object_id,
                policy,
            })
        }
        (None, None) => anyhow::bail!("pass a stored policy ID or --policy-file"),
    }
}

fn load_policy_file(path: &Path) -> anyhow::Result<Policy> {
    let bytes = std::fs::read(path)
        .map_err(|err| anyhow::anyhow!("failed to read policy file {}: {err}", path.display()))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("toml") => {
            let text = std::str::from_utf8(&bytes)
                .map_err(|err| anyhow::anyhow!("policy TOML must be UTF-8: {err}"))?;
            toml::from_str(text).map_err(|err| anyhow::anyhow!("policy file must be TOML: {err}"))
        }
        _ => serde_json::from_slice(&bytes)
            .map_err(|err| anyhow::anyhow!("policy file must be JSON or .toml: {err}")),
    }
}

fn simulate_policy_steps(
    policy: &Policy,
    revision: &Revision,
    capsule: &Capsule,
    context: &PolicyContext,
) -> Vec<SimulationStep> {
    let mut steps = Vec::new();

    steps.push(simulation_step(
        "visibility",
        "capsule private-field visibility satisfies the policy",
        serde_json::json!({
            "visibility": format!("{:?}", policy.visibility),
            "has_encrypted_private": capsule.encrypted_private.as_ref().is_some_and(|bytes| !bytes.is_empty()),
            "encryption": capsule.encryption,
            "key_id": capsule.key_id,
            "signer_key_ids": context.signer_key_ids,
        }),
        || check_visibility(policy, capsule, context),
    ));
    steps.push(simulation_step(
        "required_checks",
        "all required checks have passing capsule evidence",
        serde_json::json!({
            "required_checks": policy.required_checks,
            "evidence": capsule.public_fields.evidence.iter().map(|evidence| serde_json::json!({
                "name": evidence.name,
                "status": evidence.status,
            })).collect::<Vec<_>>(),
        }),
        || verify_required_checks(policy, capsule),
    ));
    steps.push(simulation_step(
        "authorized_recipients",
        "recipient envelopes satisfy authorized and revoked recipient policy",
        serde_json::json!({
            "authorized_recipients": policy.authorized_recipients,
            "revoked_recipients": policy.revoked_recipients,
            "capsule_recipients": capsule.recipients.iter().map(|recipient| serde_json::json!({
                "recipient_id": recipient.recipient_id,
                "key_id": recipient.key_id,
                "algorithm": recipient.algorithm,
            })).collect::<Vec<_>>(),
        }),
        || verify_authorized_recipients(policy, capsule),
    ));
    steps.push(simulation_step(
        "evidence_freshness",
        "required evidence is revision-bound, fresh, and produced by trusted runners",
        serde_json::json!({
            "policy": policy.evidence_policy,
            "revision_id": context.revision_id.map(|id| id.to_string()),
            "revision_created_at_ms": revision.created_at_ms,
            "capsule_revision_id": capsule.revision_id.to_string(),
            "now_ms": context.now_ms,
        }),
        || verify_evidence_freshness(policy, revision, capsule, context),
    ));
    steps.push(simulation_step(
        "required_reviewers",
        "required reviewer IDs are present among verified signer agents or keys",
        serde_json::json!({
            "required_reviewers": policy.required_reviewers,
            "signer_agent_ids": context.signer_agent_ids,
            "signer_key_ids": context.signer_key_ids,
        }),
        || verify_required_reviewers(policy, context),
    ));
    steps.push(simulation_step(
        "sensitive_paths",
        "sensitive touched paths have required private capsule metadata",
        serde_json::json!({
            "sensitive_paths": policy.sensitive_paths,
            "touched_paths": context.touched_paths,
            "has_encrypted_private": capsule.encrypted_private.as_ref().is_some_and(|bytes| !bytes.is_empty()),
        }),
        || verify_sensitive_paths(policy, capsule, context),
    ));
    steps.push(simulation_step(
        "quarantine_lane",
        "quarantine lane rules allow this automated evaluation context",
        serde_json::json!({
            "quarantine_lane": policy.quarantine_lane,
            "sensitive_paths": policy.sensitive_paths,
            "touched_paths": context.touched_paths,
        }),
        || verify_quarantine_lane(policy, context),
    ));
    steps.push(simulation_step(
        "min_trust_score",
        "evaluated trust score meets the policy threshold",
        serde_json::json!({
            "min_trust_score": policy.min_trust_score,
            "trust_score": context.trust_score,
        }),
        || verify_min_trust_score(policy, context),
    ));
    steps.push(simulation_step(
        "external_plugins",
        "all configured external policy plugins allow the evaluation",
        serde_json::json!({
            "configured_by": "CLAW_POLICY_PLUGINS",
        }),
        || evaluate_plugins(policy, revision, capsule, context),
    ));

    steps
}

fn simulation_step<F>(
    name: &'static str,
    description: &'static str,
    inputs: serde_json::Value,
    check: F,
) -> SimulationStep
where
    F: FnOnce() -> Result<(), claw_policy::PolicyError>,
{
    match check() {
        Ok(()) => SimulationStep {
            name,
            description,
            passed: true,
            reason: None,
            inputs,
        },
        Err(err) => SimulationStep {
            name,
            description,
            passed: false,
            reason: Some(err.to_string()),
            inputs,
        },
    }
}

fn print_simulation_steps(steps: &[SimulationStep]) {
    println!("Policy simulation:");
    let failed_steps = simulation_failures(steps);
    println!(
        "  Summary: {} passed, {} failed",
        steps.len().saturating_sub(failed_steps.len()),
        failed_steps.len()
    );
    if let Some(first) = failed_steps.first() {
        println!("  First failure: {} - {}", first.step, first.reason);
    }
    for step in steps {
        let state = if step.passed { "pass" } else { "fail" };
        match &step.reason {
            Some(reason) => println!("  {state}: {} - {reason}", step.name),
            None => println!("  {state}: {}", step.name),
        }
    }
}

fn derive_revision_touched_paths(
    store: &ClawStore,
    revision: &Revision,
) -> anyhow::Result<Vec<String>> {
    let mut paths = Vec::new();
    for patch_id in &revision.patches {
        let Object::Patch(patch) = store.load_object(patch_id)? else {
            anyhow::bail!("revision patch ref does not point to a patch object: {patch_id}");
        };
        if !paths.iter().any(|path| path == &patch.target_path) {
            paths.push(patch.target_path);
        }
    }

    if paths.is_empty() {
        let parent_tree = revision.parents.first().and_then(|parent_id| {
            match store.load_object(parent_id).ok()? {
                Object::Revision(parent) => parent.tree,
                _ => None,
            }
        });
        for change in diff_trees(store, parent_tree.as_ref(), revision.tree.as_ref(), "")? {
            if !paths.iter().any(|path| path == &change.path) {
                paths.push(change.path);
            }
        }
    }

    Ok(paths)
}

fn simulation_failures(steps: &[SimulationStep]) -> Vec<SimulationFailure> {
    steps
        .iter()
        .filter_map(|step| {
            step.reason.as_ref().map(|reason| SimulationFailure {
                step: step.name,
                reason: reason.clone(),
            })
        })
        .collect()
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn load_policy(store: &ClawStore, id: &str) -> anyhow::Result<(String, ObjectId, Policy)> {
    let ref_name = if id.starts_with("policies/") {
        id.to_string()
    } else {
        format!("policies/{id}")
    };
    let obj_id = store
        .get_ref(&ref_name)?
        .ok_or_else(|| anyhow::anyhow!("policy not found: {ref_name}"))?;
    let obj = store.load_object(&obj_id)?;
    match obj {
        Object::Policy(policy) => Ok((ref_name, obj_id, policy)),
        _ => anyhow::bail!("ref does not point to a policy object: {ref_name}"),
    }
}

fn load_revision(store: &ClawStore, value: &str) -> anyhow::Result<(ObjectId, Revision)> {
    let id = resolve_object_ref_or_id(store, value)?;
    match store.load_object(&id)? {
        Object::Revision(revision) => Ok((id, revision)),
        _ => anyhow::bail!("not a revision: {value}"),
    }
}

fn load_capsule(store: &ClawStore, value: &str) -> anyhow::Result<(ObjectId, Capsule)> {
    let id = resolve_object_ref_or_id(store, value)?;
    load_capsule_id(store, id, value)
}

fn load_default_capsule(
    store: &ClawStore,
    revision_id: &ObjectId,
    revision: &Revision,
) -> anyhow::Result<Option<(ObjectId, Capsule)>> {
    if let Some(capsule_id) = revision.capsule_id {
        return load_capsule_id(store, capsule_id, &capsule_id.to_string()).map(Some);
    }

    for ref_name in [
        format!("capsules/by-revision/{}", revision_id.to_hex()),
        format!("capsules/{}", revision_id.to_hex()),
    ] {
        if let Some(capsule_id) = store.get_ref(&ref_name)? {
            return load_capsule_id(store, capsule_id, &ref_name).map(Some);
        }
    }

    Ok(None)
}

fn synthetic_missing_capsule(revision_id: ObjectId) -> Capsule {
    Capsule {
        revision_id,
        public_fields: CapsulePublic {
            agent_id: String::new(),
            agent_version: None,
            toolchain_digest: None,
            env_fingerprint: None,
            evidence: Vec::new(),
        },
        encrypted_private: None,
        encryption: String::new(),
        key_id: None,
        recipients: Vec::new(),
        signatures: Vec::new(),
    }
}

fn load_capsule_id(
    store: &ClawStore,
    capsule_id: ObjectId,
    source: &str,
) -> anyhow::Result<(ObjectId, Capsule)> {
    match store.load_object(&capsule_id)? {
        Object::Capsule(capsule) => Ok((capsule_id, capsule)),
        _ => anyhow::bail!("not a capsule: {source}"),
    }
}

fn resolve_object_ref_or_id(store: &ClawStore, value: &str) -> anyhow::Result<ObjectId> {
    if let Some(id) = store.get_ref(value)? {
        return Ok(id);
    }
    if let Ok(id) = ObjectId::from_hex(value) {
        return Ok(id);
    }
    if let Ok(id) = ObjectId::from_display(value) {
        return Ok(id);
    }
    anyhow::bail!("cannot resolve object or ref: {value}")
}

fn parse_visibility(value: &str) -> anyhow::Result<Visibility> {
    match value.to_ascii_lowercase().as_str() {
        "public" => Ok(Visibility::Public),
        "private" => Ok(Visibility::Private),
        "encrypted-metadata-required" | "encrypted_metadata_required" | "restricted" => {
            Ok(Visibility::EncryptedMetadataRequired)
        }
        _ => anyhow::bail!(
            "unknown visibility '{}'; expected public|private|encrypted-metadata-required",
            value
        ),
    }
}

fn parse_trust_score(value: &str) -> anyhow::Result<f32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("min trust score cannot be empty");
    }

    let parsed = if let Some(percent) = trimmed.strip_suffix('%') {
        percent
            .trim()
            .parse::<f32>()
            .map_err(|_| anyhow::anyhow!("invalid percentage trust score '{}'", value))?
            / 100.0
    } else {
        trimmed
            .parse::<f32>()
            .map_err(|_| anyhow::anyhow!("invalid trust score '{}'", value))?
    };

    if !(0.0..=1.0).contains(&parsed) {
        anyhow::bail!("min trust score '{}' must be between 0 and 1", value);
    }

    Ok(parsed)
}

fn validate_min_trust_score(value: &str) -> anyhow::Result<()> {
    parse_trust_score(value)?;
    Ok(())
}

fn derive_capsule_trust_score(capsule: &Capsule) -> Option<f32> {
    let total = capsule.public_fields.evidence.len();
    if total == 0 {
        return None;
    }

    let passed = capsule
        .public_fields
        .evidence
        .iter()
        .filter(|e| e.status.eq_ignore_ascii_case("pass"))
        .count();

    Some(passed as f32 / total as f32)
}

#[cfg(test)]
mod tests {
    use super::{
        build_policy, lint_policy, parse_visibility, validate_min_trust_score, PolicyArgs,
        PolicyBuildOptions, PolicyCommand,
    };
    use clap::Parser;
    use claw_core::types::Visibility;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: PolicyArgs,
    }

    #[test]
    fn parses_apply_dry_run() {
        let cli = TestCli::parse_from(["claw", "apply", "--id", "release", "--dry-run"]);

        match cli.args.command {
            PolicyCommand::Apply { id, dry_run, .. } => {
                assert_eq!(id, "release");
                assert!(dry_run);
            }
            _ => panic!("expected apply command"),
        }
    }

    #[test]
    fn parses_create_recipient_and_freshness_flags() {
        let cli = TestCli::parse_from([
            "claw",
            "create",
            "--id",
            "sensitive",
            "--recipient",
            "security",
            "--revoked-recipient",
            "former-reviewer",
            "--require-fresh-evidence",
            "--trusted-runner",
            "github-actions/release",
            "--evidence-max-age-ms",
            "60000",
        ]);

        match cli.args.command {
            PolicyCommand::Create {
                id,
                recipients,
                revoked_recipients,
                require_fresh_evidence,
                evidence_max_age_ms,
                trusted_runners,
                ..
            } => {
                assert_eq!(id, "sensitive");
                assert_eq!(recipients, vec!["security".to_string()]);
                assert_eq!(revoked_recipients, vec!["former-reviewer".to_string()]);
                assert!(require_fresh_evidence);
                assert_eq!(evidence_max_age_ms, Some(60_000));
                assert_eq!(trusted_runners, vec!["github-actions/release".to_string()]);
            }
            _ => panic!("expected create command"),
        }
    }

    #[test]
    fn parses_eval_json() {
        let cli = TestCli::parse_from([
            "claw",
            "eval",
            "release",
            "--revision",
            "heads/main",
            "--json",
        ]);

        match cli.args.command {
            PolicyCommand::Eval {
                id, revision, json, ..
            } => {
                assert_eq!(id.as_deref(), Some("release"));
                assert_eq!(revision, "heads/main");
                assert!(json);
            }
            _ => panic!("expected eval command"),
        }
    }

    #[test]
    fn parses_eval_policy_file_json() {
        let cli = TestCli::parse_from([
            "claw",
            "simulate",
            "--policy-file",
            "release-policy.json",
            "--revision",
            "heads/main",
            "--json",
        ]);

        match cli.args.command {
            PolicyCommand::Eval {
                id,
                policy_file,
                revision,
                json,
                ..
            } => {
                assert!(id.is_none());
                assert_eq!(
                    policy_file.as_deref().and_then(std::path::Path::to_str),
                    Some("release-policy.json")
                );
                assert_eq!(revision, "heads/main");
                assert!(json);
            }
            _ => panic!("expected eval command"),
        }
    }

    #[test]
    fn parses_lint_json() {
        let cli = TestCli::parse_from(["claw", "lint", "release", "--json"]);

        match cli.args.command {
            PolicyCommand::Lint { id, json } => {
                assert_eq!(id.as_deref(), Some("release"));
                assert!(json);
            }
            _ => panic!("expected lint command"),
        }
    }

    #[test]
    fn parses_visibility_values() {
        assert_eq!(parse_visibility("public").unwrap(), Visibility::Public);
        assert_eq!(parse_visibility("PRIVATE").unwrap(), Visibility::Private);
        assert_eq!(
            parse_visibility("encrypted-metadata-required").unwrap(),
            Visibility::EncryptedMetadataRequired
        );
        assert_eq!(
            parse_visibility("restricted").unwrap(),
            Visibility::EncryptedMetadataRequired
        );
    }

    #[test]
    fn rejects_unknown_visibility() {
        assert!(parse_visibility("secret").is_err());
    }

    #[test]
    fn validates_min_trust_score() {
        assert!(validate_min_trust_score("0.75").is_ok());
        assert!(validate_min_trust_score("80%").is_ok());
        assert!(validate_min_trust_score("1.5").is_err());
        assert!(validate_min_trust_score("abc").is_err());
    }

    #[test]
    fn build_policy_preserves_default_freshness_max_age() {
        let policy = build_policy(PolicyBuildOptions {
            id: "fresh".to_string(),
            visibility: "public".to_string(),
            checks: vec!["test".to_string()],
            reviewers: vec![],
            sensitive_paths: vec![],
            quarantine_lane: false,
            min_trust_score: None,
            recipients: vec![],
            revoked_recipients: vec![],
            require_fresh_evidence: true,
            evidence_max_age_ms: None,
            trusted_runners: vec![],
        })
        .unwrap();

        assert!(policy.evidence_policy.require_fresh_evidence);
        assert_eq!(
            policy.evidence_policy.max_age_ms,
            Some(24 * 60 * 60 * 1_000)
        );
    }

    #[test]
    fn lint_flags_dangerous_policy_shapes() {
        let policy = build_policy(PolicyBuildOptions {
            id: "dangerous".to_string(),
            visibility: "public".to_string(),
            checks: vec![],
            reviewers: vec![],
            sensitive_paths: vec!["secrets/**".to_string()],
            quarantine_lane: false,
            min_trust_score: None,
            recipients: vec!["security".to_string()],
            revoked_recipients: vec!["SECURITY".to_string()],
            require_fresh_evidence: true,
            evidence_max_age_ms: None,
            trusted_runners: vec![],
        })
        .unwrap();

        let findings = lint_policy(&policy);
        let codes = findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&"FRESHNESS_WITHOUT_REQUIRED_CHECKS"));
        assert!(codes.contains(&"FRESHNESS_WITHOUT_TRUSTED_RUNNER"));
        assert!(codes.contains(&"RECIPIENT_AUTH_REVOKE_OVERLAP"));
    }

    #[test]
    fn lint_flags_noop_policy() {
        let policy = build_policy(PolicyBuildOptions {
            id: "noop".to_string(),
            visibility: "public".to_string(),
            checks: vec![],
            reviewers: vec![],
            sensitive_paths: vec![],
            quarantine_lane: false,
            min_trust_score: None,
            recipients: vec![],
            revoked_recipients: vec![],
            require_fresh_evidence: false,
            evidence_max_age_ms: None,
            trusted_runners: vec![],
        })
        .unwrap();

        let findings = lint_policy(&policy);
        assert!(findings
            .iter()
            .any(|finding| finding.code == "POLICY_NO_ENFORCEMENT"));
    }
}
