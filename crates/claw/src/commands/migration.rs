use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use claw_core::id::{ChangeId, IntentId, ObjectId};
use claw_core::object::Object;
use claw_core::types::{
    Blob, Change, ChangeStatus, EvidencePolicy, Intent, IntentStatus, Policy, Visibility,
};
use claw_git::importer::{list_git_refs, GitImporter};
use claw_store::ClawStore;

use super::git_notes::{import_note_into_store, read_note};
use crate::config::find_repo_root;

#[derive(Args)]
pub struct MigrationArgs {
    #[command(subcommand)]
    command: MigrationCommand,
}

#[derive(Subcommand)]
enum MigrationCommand {
    /// Import a Git repo and infer Claw migration objects
    Wizard {
        /// Path to source .git directory
        #[arg(long, default_value = ".git")]
        git_dir: String,
        /// Destination Claw ref prefix for imported Git branches
        #[arg(long, default_value = "heads/migrated/")]
        head_prefix: String,
        /// Import Claw provenance from Git notes
        #[arg(long)]
        read_notes: bool,
        /// Git notes ref used when --read-notes is enabled
        #[arg(long, default_value = "claw")]
        notes_ref: String,
        /// GitHub/GitLab issue, PR, MR, checks, or branch-protection JSON export
        #[arg(long)]
        metadata_file: Option<String>,
        /// Preview the migration plan without writing Claw objects or refs
        #[arg(long)]
        dry_run: bool,
        /// Output the migration report as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
struct MigrationReport {
    schema_version: u8,
    action: &'static str,
    dry_run: bool,
    git_dir: String,
    branch_count: usize,
    branches: Vec<BranchMigrationReport>,
    metadata_summary: MetadataSummaryReport,
    notes_imported: usize,
    metadata_ref: Option<String>,
    policy_suggestion_ref: Option<String>,
    suggested_policy: SuggestedPolicyReport,
}

#[derive(Debug, Serialize)]
struct BranchMigrationReport {
    git_ref: String,
    claw_ref: String,
    revision_id: Option<String>,
    intent_id: String,
    change_id: String,
    title: String,
    metadata_match: &'static str,
    metadata_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MetadataSummaryReport {
    branch_metadata_records: usize,
    issue_metadata_records: usize,
    branch_protection_records: usize,
    required_check_count: usize,
    required_reviewer_count: usize,
    required_approving_review_count: Option<u64>,
    require_code_owner_reviews: bool,
    dismiss_stale_reviews: bool,
    require_last_push_approval: bool,
    matched_by_branch: usize,
    matched_by_issue: usize,
    fallback_branches: usize,
}

#[derive(Debug, Clone, Serialize)]
struct SuggestedPolicyReport {
    policy_id: String,
    required_checks: Vec<String>,
    required_reviewers: Vec<String>,
    sensitive_paths: Vec<String>,
    min_trust_score: String,
    review_requirements: ReviewRequirementsReport,
    migration_warnings: Vec<String>,
    policy_object_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ReviewRequirementsReport {
    required_approving_review_count: Option<u64>,
    require_code_owner_reviews: bool,
    dismiss_stale_reviews: bool,
    require_last_push_approval: bool,
}

#[derive(Debug, Clone)]
struct BranchMetadata {
    title: String,
    body: String,
    links: Vec<String>,
}

struct InferredBranchMetadata {
    metadata: BranchMetadata,
    match_kind: &'static str,
}

#[derive(Debug, Default)]
struct MetadataIndex {
    by_branch: HashMap<String, BranchMetadata>,
    by_issue_number: HashMap<String, BranchMetadata>,
    required_checks: BTreeSet<String>,
    required_reviewers: BTreeSet<String>,
    branch_protection_records: usize,
    review_requirements: ReviewRequirementsReport,
}

pub fn run(args: MigrationArgs) -> anyhow::Result<()> {
    match args.command {
        MigrationCommand::Wizard {
            git_dir,
            head_prefix,
            read_notes,
            notes_ref,
            metadata_file,
            dry_run,
            json,
        } => run_wizard(MigrationWizardOptions {
            git_dir,
            head_prefix,
            read_notes,
            notes_ref,
            metadata_file,
            dry_run,
            json,
        }),
    }
}

struct MigrationWizardOptions {
    git_dir: String,
    head_prefix: String,
    read_notes: bool,
    notes_ref: String,
    metadata_file: Option<String>,
    dry_run: bool,
    json: bool,
}

fn run_wizard(options: MigrationWizardOptions) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let git_dir = root.join(&options.git_dir);
    let head_prefix = normalize_head_prefix(&options.head_prefix);
    validate_ref_path(head_prefix.trim_end_matches('/'))?;

    let (metadata_value, metadata_index) =
        load_metadata_index(options.metadata_file.as_deref().map(|path| root.join(path)))?;
    let refs = list_git_refs(&git_dir, "refs/heads/")?;
    if refs.is_empty() {
        anyhow::bail!("no git branches found under refs/heads/");
    }

    let mut importer = if options.dry_run {
        None
    } else {
        Some(GitImporter::new(&store))
    };
    let mut branches = Vec::new();

    for (git_ref, _sha) in refs {
        let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(&git_ref);
        let claw_ref = format!("{head_prefix}{branch}");
        validate_ref_path(&claw_ref)?;
        let inferred = infer_branch_metadata(branch, &metadata_index);
        let intent_id = deterministic_intent_id("migration-intent", branch);
        let change_id = deterministic_change_id("migration-change", branch);

        let revision_id = if options.dry_run {
            None
        } else {
            let importer = importer
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("internal migration importer state missing"))?;
            let revision_id = importer.import_ref(&git_dir, &git_ref, &claw_ref)?;
            write_inferred_intent_change(
                &store,
                intent_id,
                change_id,
                revision_id,
                branch,
                &inferred.metadata,
                &metadata_index,
            )?;
            Some(revision_id)
        };

        branches.push(BranchMigrationReport {
            git_ref,
            claw_ref,
            revision_id: revision_id.map(|id| id.to_string()),
            intent_id: intent_id.to_string(),
            change_id: change_id.to_string(),
            title: inferred.metadata.title,
            metadata_match: inferred.match_kind,
            metadata_links: inferred.metadata.links,
        });
    }

    let notes_imported = if options.read_notes && !options.dry_run {
        let importer = importer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("internal migration importer state missing"))?;
        import_notes_for_imported_commits(&store, importer, &git_dir, &options.notes_ref)?
    } else {
        0
    };

    let metadata_ref = if options.dry_run {
        metadata_value
            .as_ref()
            .map(|value| {
                Ok::<_, anyhow::Error>(format!(
                    "migration/metadata/{}",
                    value_digest_prefix(value)?
                ))
            })
            .transpose()?
    } else {
        store_metadata(&store, metadata_value.as_ref())?
    };

    let mut suggested_policy = build_suggested_policy_report(&metadata_index);
    let policy_suggestion_ref = if options.dry_run {
        Some(format!(
            "migration/policy-suggestions/{}",
            suggestion_digest_prefix(&suggested_policy)?
        ))
    } else {
        let (suggestion_ref, policy_object_id) =
            store_policy_suggestions(&store, &suggested_policy)?;
        suggested_policy.policy_object_id = Some(policy_object_id.to_string());
        Some(suggestion_ref)
    };

    let report = MigrationReport {
        schema_version: 1,
        action: "migration.wizard",
        dry_run: options.dry_run,
        git_dir: git_dir.display().to_string(),
        branch_count: branches.len(),
        metadata_summary: metadata_summary(&metadata_index, &branches),
        branches,
        notes_imported,
        metadata_ref,
        policy_suggestion_ref,
        suggested_policy,
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, options.read_notes, &options.notes_ref);
    }

    Ok(())
}

fn print_human_report(report: &MigrationReport, read_notes: bool, notes_ref: &str) {
    if report.dry_run {
        println!("Dry run: would run team migration wizard");
    } else {
        println!("Team migration complete");
    }
    println!("  Git dir: {}", report.git_dir);
    println!("  Branches: {}", report.branch_count);
    for branch in &report.branches {
        println!("  {} -> {}", branch.git_ref, branch.claw_ref);
        println!("    Intent: {} ({})", branch.intent_id, branch.title);
        println!("    Change: {}", branch.change_id);
        println!("    Metadata match: {}", branch.metadata_match);
        if let Some(revision_id) = branch.revision_id.as_deref() {
            println!("    Revision: {revision_id}");
        }
        for link in &branch.metadata_links {
            println!("    Link: {link}");
        }
    }
    if read_notes {
        if report.dry_run {
            println!("  Git notes: would scan refs/notes/{notes_ref}");
        } else {
            println!("  Git notes imported: {}", report.notes_imported);
        }
    }
    if let Some(metadata_ref) = report.metadata_ref.as_deref() {
        println!("  Metadata: {metadata_ref}");
    }
    if let Some(suggestion_ref) = report.policy_suggestion_ref.as_deref() {
        println!("  Policy suggestions: {suggestion_ref}");
    }
    println!(
        "  Metadata coverage: branch_matches={}, issue_matches={}, fallbacks={}",
        report.metadata_summary.matched_by_branch,
        report.metadata_summary.matched_by_issue,
        report.metadata_summary.fallback_branches
    );
    println!(
        "  Suggested policy {}: checks={}, reviewers={}, sensitive_paths={}, min_trust_score={}",
        report.suggested_policy.policy_id,
        report.suggested_policy.required_checks.len(),
        report.suggested_policy.required_reviewers.len(),
        report.suggested_policy.sensitive_paths.len(),
        report.suggested_policy.min_trust_score
    );
    if let Some(count) = report
        .suggested_policy
        .review_requirements
        .required_approving_review_count
    {
        println!("  Review requirement: {count} approving review(s)");
    }
    if report
        .suggested_policy
        .review_requirements
        .require_code_owner_reviews
    {
        println!("  Review requirement: code owner reviews");
    }
    for warning in &report.suggested_policy.migration_warnings {
        println!("  Warning: {warning}");
    }
}

fn write_inferred_intent_change(
    store: &ClawStore,
    intent_id: IntentId,
    change_id: ChangeId,
    revision_id: ObjectId,
    branch: &str,
    metadata: &BranchMetadata,
    metadata_index: &MetadataIndex,
) -> anyhow::Result<()> {
    let now = now_ms()?;
    let policy_refs = vec!["migration-suggested".to_string()];
    let intent = Intent {
        id: intent_id,
        title: metadata.title.clone(),
        goal: if metadata.body.trim().is_empty() {
            format!("Migrated from Git branch `{branch}`.")
        } else {
            metadata.body.clone()
        },
        constraints: vec![
            format!("Source Git branch: {branch}"),
            "Review generated policy suggestions before enforcing them.".to_string(),
        ],
        acceptance_tests: metadata_index
            .required_checks
            .iter()
            .map(|check| format!("evidence:{check}=pass"))
            .collect(),
        links: metadata.links.clone(),
        policy_refs,
        agents: Vec::new(),
        change_ids: vec![change_id.to_string()],
        depends_on: Vec::new(),
        supersedes: Vec::new(),
        status: IntentStatus::Open,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let intent_id_obj = store.store_object(&Object::Intent(intent))?;
    store.set_ref(&format!("intents/{intent_id}"), &intent_id_obj)?;

    let change = Change {
        id: change_id,
        intent_id,
        head_revision: Some(revision_id),
        workstream_id: Some("git-migration".to_string()),
        status: ChangeStatus::Ready,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let change_id_obj = store.store_object(&Object::Change(change))?;
    store.set_ref(&format!("changes/{change_id}"), &change_id_obj)?;
    Ok(())
}

fn load_metadata_index(path: Option<PathBuf>) -> anyhow::Result<(Option<Value>, MetadataIndex)> {
    let Some(path) = path else {
        return Ok((None, MetadataIndex::default()));
    };
    let data = std::fs::read_to_string(&path)?;
    let value: Value = serde_json::from_str(&data)
        .map_err(|err| anyhow::anyhow!("metadata file is not valid JSON: {err}"))?;
    let mut index = MetadataIndex::default();
    index_value(&value, &mut index);
    Ok((Some(value), index))
}

fn index_value(value: &Value, index: &mut MetadataIndex) {
    match value {
        Value::Array(items) => {
            for item in items {
                index_value(item, index);
            }
        }
        Value::Object(map) => {
            if let Some(check) = first_string_field(value, &["context", "name", "check_name"]) {
                if looks_like_check_object(value) {
                    index.required_checks.insert(check);
                }
            }
            collect_string_array_fields(
                value,
                &["required_checks", "required_status_checks", "contexts"],
                &mut index.required_checks,
            );
            collect_string_array_fields(
                value,
                &["required_reviewers", "reviewers", "approvers"],
                &mut index.required_reviewers,
            );
            collect_branch_protection(value, index);
            if let Some(login) = value.pointer("/user/login").and_then(Value::as_str) {
                if map.contains_key("state") && map.contains_key("submitted_at") {
                    index.required_reviewers.insert(login.to_string());
                }
            }
            if let Some(branch) = branch_name_from_record(value) {
                let metadata = branch_metadata_from_record(value);
                if value.get("title").is_some() || value.get("name").is_some() {
                    index.by_branch.insert(branch, metadata);
                }
            } else if let Some(number) = issue_number_from_record(value) {
                let metadata = branch_metadata_from_record(value);
                index.by_issue_number.insert(number, metadata);
            }
            for child in map.values() {
                index_value(child, index);
            }
        }
        _ => {}
    }
}

fn infer_branch_metadata(branch: &str, index: &MetadataIndex) -> InferredBranchMetadata {
    if let Some(metadata) = index.by_branch.get(branch) {
        return InferredBranchMetadata {
            metadata: metadata.clone(),
            match_kind: "branch",
        };
    }
    for token in branch
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        if let Some(metadata) = index.by_issue_number.get(token) {
            return InferredBranchMetadata {
                metadata: metadata.clone(),
                match_kind: "issue",
            };
        }
    }

    InferredBranchMetadata {
        metadata: BranchMetadata {
            title: format!("Migrate branch {branch}"),
            body: format!("Migrated from Git branch `{branch}`."),
            links: Vec::new(),
        },
        match_kind: "fallback",
    }
}

fn metadata_summary(
    index: &MetadataIndex,
    branches: &[BranchMigrationReport],
) -> MetadataSummaryReport {
    MetadataSummaryReport {
        branch_metadata_records: index.by_branch.len(),
        issue_metadata_records: index.by_issue_number.len(),
        branch_protection_records: index.branch_protection_records,
        required_check_count: index.required_checks.len(),
        required_reviewer_count: index.required_reviewers.len(),
        required_approving_review_count: index.review_requirements.required_approving_review_count,
        require_code_owner_reviews: index.review_requirements.require_code_owner_reviews,
        dismiss_stale_reviews: index.review_requirements.dismiss_stale_reviews,
        require_last_push_approval: index.review_requirements.require_last_push_approval,
        matched_by_branch: branches
            .iter()
            .filter(|branch| branch.metadata_match == "branch")
            .count(),
        matched_by_issue: branches
            .iter()
            .filter(|branch| branch.metadata_match == "issue")
            .count(),
        fallback_branches: branches
            .iter()
            .filter(|branch| branch.metadata_match == "fallback")
            .count(),
    }
}

fn collect_branch_protection(value: &Value, index: &mut MetadataIndex) {
    let has_branch_protection_container = value.get("required_pull_request_reviews").is_some()
        || value.get("required_status_checks").is_some()
        || value.get("branch_protection").is_some();
    let has_review_rules = value.get("required_pull_request_reviews").is_some()
        || value.get("required_approving_review_count").is_some()
        || value.get("approvals_required").is_some()
        || value.get("require_code_owner_reviews").is_some()
        || value.get("dismiss_stale_reviews").is_some()
        || value.get("require_last_push_approval").is_some();
    if !has_branch_protection_container || !has_review_rules {
        return;
    }

    index.branch_protection_records += 1;
    let required_reviews = value
        .pointer("/required_pull_request_reviews/required_approving_review_count")
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get("required_approving_review_count")
                .and_then(Value::as_u64)
        })
        .or_else(|| value.get("approvals_required").and_then(Value::as_u64));
    if let Some(count) = required_reviews {
        index.review_requirements.required_approving_review_count = Some(
            index
                .review_requirements
                .required_approving_review_count
                .map_or(count, |existing| existing.max(count)),
        );
    }

    index.review_requirements.require_code_owner_reviews |= value
        .pointer("/required_pull_request_reviews/require_code_owner_reviews")
        .and_then(Value::as_bool)
        .or_else(|| {
            value
                .get("require_code_owner_reviews")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false);
    index.review_requirements.dismiss_stale_reviews |= value
        .pointer("/required_pull_request_reviews/dismiss_stale_reviews")
        .and_then(Value::as_bool)
        .or_else(|| value.get("dismiss_stale_reviews").and_then(Value::as_bool))
        .unwrap_or(false);
    index.review_requirements.require_last_push_approval |= value
        .pointer("/required_pull_request_reviews/require_last_push_approval")
        .and_then(Value::as_bool)
        .or_else(|| {
            value
                .get("require_last_push_approval")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false);
}

fn branch_name_from_record(value: &Value) -> Option<String> {
    first_string_field(
        value,
        &[
            "source_branch",
            "sourceBranch",
            "head_ref",
            "headRefName",
            "branch",
            "ref",
        ],
    )
    .or_else(|| {
        value
            .pointer("/head/ref")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

fn issue_number_from_record(value: &Value) -> Option<String> {
    if !value.get("title").is_some() {
        return None;
    }
    first_string_field(value, &["number", "iid", "id"]).or_else(|| {
        value
            .get("number")
            .and_then(Value::as_i64)
            .map(|number| number.to_string())
    })
}

fn branch_metadata_from_record(value: &Value) -> BranchMetadata {
    let title = first_string_field(value, &["title", "name"])
        .unwrap_or_else(|| "Imported Git work".to_string());
    let body = first_string_field(value, &["body", "description"])
        .unwrap_or_else(|| "Imported from Git provider metadata.".to_string());
    let mut links = Vec::new();
    for key in ["html_url", "web_url", "url"] {
        if let Some(link) = value.get(key).and_then(Value::as_str) {
            links.push(link.to_string());
        }
    }
    links.sort();
    links.dedup();
    BranchMetadata { title, body, links }
}

fn first_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(raw) = value.get(*key) else {
            continue;
        };
        if let Some(text) = raw.as_str() {
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
        if let Some(number) = raw.as_i64() {
            return Some(number.to_string());
        }
    }
    None
}

fn collect_string_array_fields(value: &Value, keys: &[&str], out: &mut BTreeSet<String>) {
    for key in keys {
        let Some(raw) = value.get(*key) else {
            continue;
        };
        collect_strings(raw, out);
    }
}

fn collect_strings(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(text) if !text.trim().is_empty() => {
            out.insert(text.trim().to_string());
        }
        Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Object(map) => {
            for key in ["context", "name", "check_name"] {
                if let Some(text) = map.get(key).and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        out.insert(text.trim().to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

fn looks_like_check_object(value: &Value) -> bool {
    value.get("status").is_some()
        || value.get("conclusion").is_some()
        || value.get("required").is_some()
        || value.get("app").is_some()
}

fn build_suggested_policy_report(index: &MetadataIndex) -> SuggestedPolicyReport {
    let required_checks = if index.required_checks.is_empty() {
        vec!["migration/manual-review".to_string()]
    } else {
        index.required_checks.iter().cloned().collect()
    };
    let required_reviewers = if index.required_reviewers.is_empty() {
        vec!["maintainer".to_string()]
    } else {
        index.required_reviewers.iter().cloned().collect()
    };
    SuggestedPolicyReport {
        policy_id: "migration-suggested".to_string(),
        required_checks,
        required_reviewers,
        sensitive_paths: vec![
            ".github/**".to_string(),
            "infra/**".to_string(),
            "terraform/**".to_string(),
            "k8s/**".to_string(),
        ],
        min_trust_score: "0.8".to_string(),
        review_requirements: index.review_requirements.clone(),
        migration_warnings: migration_warnings(index),
        policy_object_id: None,
    }
}

fn migration_warnings(index: &MetadataIndex) -> Vec<String> {
    let mut warnings = Vec::new();
    if index
        .review_requirements
        .required_approving_review_count
        .is_some()
    {
        warnings.push(
            "Git branch protection required approving review counts; Claw stores this as a migration suggestion for human policy review."
                .to_string(),
        );
    }
    if index.review_requirements.require_code_owner_reviews {
        warnings.push(
            "Git branch protection required code owner reviews; add equivalent Claw reviewer policy before enforcing migrated work."
                .to_string(),
        );
    }
    if index.review_requirements.dismiss_stale_reviews {
        warnings.push(
            "Git branch protection dismissed stale reviews; verify stale-approval handling in the migrated workflow."
                .to_string(),
        );
    }
    if index.review_requirements.require_last_push_approval {
        warnings.push(
            "Git branch protection required last-push approval; preserve this gate in the team migration plan."
                .to_string(),
        );
    }
    warnings
}

fn store_policy_suggestions(
    store: &ClawStore,
    suggestion: &SuggestedPolicyReport,
) -> anyhow::Result<(String, ObjectId)> {
    let blob = Object::Blob(Blob {
        data: serde_json::to_vec_pretty(suggestion)?,
        media_type: Some("application/json".to_string()),
    });
    let blob_id = store.store_object(&blob)?;
    let suggestion_ref = format!(
        "migration/policy-suggestions/{}",
        suggestion_digest_prefix(suggestion)?
    );
    store.set_ref(&suggestion_ref, &blob_id)?;

    let policy = Policy {
        policy_id: suggestion.policy_id.clone(),
        required_checks: suggestion.required_checks.clone(),
        required_reviewers: suggestion.required_reviewers.clone(),
        sensitive_paths: suggestion.sensitive_paths.clone(),
        quarantine_lane: true,
        min_trust_score: Some(suggestion.min_trust_score.clone()),
        visibility: Visibility::Public,
        authorized_recipients: Vec::new(),
        revoked_recipients: Vec::new(),
        evidence_policy: EvidencePolicy {
            require_fresh_evidence: true,
            trusted_runner_identities: suggestion.required_reviewers.clone(),
            ..EvidencePolicy::default()
        },
    };
    let policy_id = store.store_object(&Object::Policy(policy))?;
    store.set_ref(&format!("policies/{}", suggestion.policy_id), &policy_id)?;
    Ok((suggestion_ref, policy_id))
}

fn store_metadata(store: &ClawStore, metadata: Option<&Value>) -> anyhow::Result<Option<String>> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let blob = Object::Blob(Blob {
        data: serde_json::to_vec_pretty(metadata)?,
        media_type: Some("application/json".to_string()),
    });
    let blob_id = store.store_object(&blob)?;
    let ref_name = format!("migration/metadata/{}", value_digest_prefix(metadata)?);
    store.set_ref(&ref_name, &blob_id)?;
    Ok(Some(ref_name))
}

fn import_notes_for_imported_commits(
    store: &ClawStore,
    importer: &GitImporter<'_>,
    git_dir: &Path,
    notes_ref: &str,
) -> anyhow::Result<usize> {
    let mut imported = 0usize;
    for (commit_sha, revision_id) in importer.imported_commits() {
        let commit_hex = hex::encode(commit_sha);
        let Some(note) = read_note(git_dir, notes_ref, &commit_hex)? else {
            continue;
        };
        import_note_into_store(store, &revision_id, note)?;
        imported += 1;
    }
    Ok(imported)
}

fn deterministic_intent_id(kind: &str, branch: &str) -> IntentId {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&stable_digest(kind, branch)[..16]);
    IntentId::from_bytes(bytes)
}

fn deterministic_change_id(kind: &str, branch: &str) -> ChangeId {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&stable_digest(kind, branch)[..16]);
    ChangeId::from_bytes(bytes)
}

fn stable_digest(kind: &str, branch: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(branch.as_bytes());
    hasher.finalize().into()
}

fn value_digest_prefix(value: &Value) -> anyhow::Result<String> {
    let data = serde_json::to_vec(value)?;
    Ok(hex_digest_prefix(&data))
}

fn suggestion_digest_prefix(suggestion: &SuggestedPolicyReport) -> anyhow::Result<String> {
    let data = serde_json::to_vec(suggestion)?;
    Ok(hex_digest_prefix(&data))
}

fn hex_digest_prefix(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest: [u8; 32] = hasher.finalize().into();
    hex::encode(&digest[..8])
}

fn now_ms() -> anyhow::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64)
}

fn normalize_head_prefix(prefix: &str) -> String {
    if prefix.is_empty() {
        return "heads/migrated/".to_string();
    }
    if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    }
}

fn validate_ref_path(ref_name: &str) -> anyhow::Result<()> {
    let path = Path::new(ref_name);
    if path.is_absolute() {
        anyhow::bail!("invalid ref '{}': must be relative", ref_name);
    }

    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => {}
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                anyhow::bail!(
                    "invalid ref '{}': cannot contain '.', '..', or root components",
                    ref_name
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::json;

    use super::{
        build_suggested_policy_report, deterministic_change_id, deterministic_intent_id,
        index_value, infer_branch_metadata, normalize_head_prefix, validate_ref_path,
        MetadataIndex, MigrationArgs, MigrationCommand,
    };

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: MigrationArgs,
    }

    #[test]
    fn parses_wizard_dry_run_json() {
        let cli = TestCli::parse_from([
            "claw",
            "wizard",
            "--git-dir",
            "../source/.git",
            "--metadata-file",
            "provider.json",
            "--read-notes",
            "--dry-run",
            "--json",
        ]);

        match cli.args.command {
            MigrationCommand::Wizard {
                git_dir,
                metadata_file,
                read_notes,
                dry_run,
                json,
                ..
            } => {
                assert_eq!(git_dir, "../source/.git");
                assert_eq!(metadata_file.as_deref(), Some("provider.json"));
                assert!(read_notes);
                assert!(dry_run);
                assert!(json);
            }
        }
    }

    #[test]
    fn metadata_index_matches_pr_branch_and_checks() {
        let value = json!({
            "pull_requests": [{
                "title": "Add login",
                "body": "Implements login",
                "head": {"ref": "feature/login"},
                "html_url": "https://example.test/pull/1",
                "checks": [{"name": "ci", "status": "completed"}],
                "reviews": [{"user": {"login": "alice"}, "state": "APPROVED", "submitted_at": "now"}]
            }],
            "branch_protection": {
                "required_status_checks": {"contexts": ["lint"]},
                "required_pull_request_reviews": {
                    "required_approving_review_count": 2,
                    "require_code_owner_reviews": true,
                    "dismiss_stale_reviews": true
                }
            }
        });
        let mut index = MetadataIndex::default();
        index_value(&value, &mut index);

        let metadata = infer_branch_metadata("feature/login", &index);
        assert_eq!(metadata.match_kind, "branch");
        assert_eq!(metadata.metadata.title, "Add login");
        assert!(metadata
            .metadata
            .links
            .contains(&"https://example.test/pull/1".to_string()));
        assert!(index.required_checks.contains("ci"));
        assert!(index.required_checks.contains("lint"));
        assert!(index.required_reviewers.contains("alice"));
        assert_eq!(index.branch_protection_records, 1);
        assert_eq!(
            index.review_requirements.required_approving_review_count,
            Some(2)
        );
        assert!(index.review_requirements.require_code_owner_reviews);
        assert!(index.review_requirements.dismiss_stale_reviews);

        let suggested = build_suggested_policy_report(&index);
        assert_eq!(
            suggested
                .review_requirements
                .required_approving_review_count,
            Some(2)
        );
        assert!(suggested.review_requirements.require_code_owner_reviews);
        assert!(suggested
            .migration_warnings
            .iter()
            .any(|warning| warning.contains("code owner reviews")));
    }

    #[test]
    fn deterministic_ids_are_stable_and_distinct() {
        assert_eq!(
            deterministic_intent_id("migration-intent", "main").to_string(),
            deterministic_intent_id("migration-intent", "main").to_string()
        );
        assert_ne!(
            deterministic_intent_id("migration-intent", "main").to_string(),
            deterministic_change_id("migration-change", "main").to_string()
        );
    }

    #[test]
    fn validates_ref_paths() {
        assert_eq!(normalize_head_prefix("heads/imported"), "heads/imported/");
        assert!(validate_ref_path("heads/imported/main").is_ok());
        assert!(validate_ref_path("heads/../main").is_err());
        assert!(validate_ref_path("/absolute").is_err());
    }
}
