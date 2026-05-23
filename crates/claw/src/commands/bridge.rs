use std::path::PathBuf;

use clap::{Args, Subcommand};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Serialize;
use sha2::{Digest, Sha256};

use claw_core::id::{ChangeId, IntentId, ObjectId};
use claw_core::object::Object;
use claw_core::types::{
    Blob, CapsulePublic, Change, ChangeStatus, Evidence, EvidencePolicy, Intent, IntentStatus,
    Policy, Visibility,
};
use claw_crypto::capsule::{append_capsule_signature, build_capsule};
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::agent::{ensure_registered_signing_agent, keypair_for_agent};
use super::object_refs::{current_time_ms, load_default_capsule, load_revision};

#[derive(Args)]
pub struct BridgeArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: BridgeCommand,
}

#[derive(Subcommand)]
enum BridgeCommand {
    /// Import GitHub/GitLab PR metadata into Claw objects
    Import {
        /// Provider name: github or gitlab
        #[arg(long)]
        provider: String,
        /// JSON export from the provider API or a normalized bridge bundle
        #[arg(long)]
        file: Option<PathBuf>,
        /// GitHub owner or GitLab namespace/group path for live fetches
        #[arg(long)]
        owner: Option<String>,
        /// GitHub repository name for live fetches
        #[arg(long)]
        repo: Option<String>,
        /// GitLab project path or numeric project id for live fetches
        #[arg(long)]
        project: Option<String>,
        /// Pull request or merge request number/IID for live fetches
        #[arg(long)]
        pull: Option<String>,
        /// Commit SHA to fetch checks/statuses for. Defaults to provider PR/MR head SHA.
        #[arg(long)]
        commit_sha: Option<String>,
        /// Provider API base URL for live fetches.
        #[arg(long)]
        base_url: Option<String>,
        /// Bearer/private token for live fetches.
        #[arg(long)]
        token: Option<String>,
        /// Environment variable containing the provider token for live fetches.
        #[arg(long)]
        token_env: Option<String>,
        /// Revision that provider checks/reviews/statuses apply to
        #[arg(long)]
        revision: String,
        /// Agent that signs created or updated capsules
        #[arg(long, default_value = "claw")]
        agent: String,
        /// Preview object/ref changes without writing them
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Serialize)]
struct BridgeImportReport {
    schema_version: u8,
    action: &'static str,
    provider: String,
    dry_run: bool,
    revision: String,
    raw_import_ref: Option<String>,
    intent: Option<BridgeObjectRef>,
    change: Option<BridgeObjectRef>,
    capsule: Option<BridgeObjectRef>,
    policy: Option<BridgeObjectRef>,
    evidence_added: usize,
    notes_imported: usize,
    mapping: BridgeMappingReport,
}

#[derive(Debug, Serialize)]
struct BridgeObjectRef {
    object: String,
    ref_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct BridgeMappingReport {
    pull_request_present: bool,
    check_count: usize,
    status_count: usize,
    review_count: usize,
    note_count: usize,
    branch_protection_present: bool,
    required_check_count: usize,
    required_reviewer_count: usize,
    required_approving_review_count: Option<u64>,
    require_code_owner_reviews: bool,
    dismiss_stale_reviews: bool,
    require_last_push_approval: bool,
}

#[derive(Debug, Clone, Default)]
struct BranchProtectionReviewRules {
    required_approving_review_count: Option<u64>,
    require_code_owner_reviews: bool,
    dismiss_stale_reviews: bool,
    require_last_push_approval: bool,
}

struct BridgePr {
    key: String,
    title: String,
    body: String,
    state: String,
    url: Option<String>,
    source_branch: Option<String>,
    target_branch: Option<String>,
}

pub async fn run(args: BridgeArgs) -> anyhow::Result<()> {
    match args.command {
        BridgeCommand::Import {
            provider,
            file,
            owner,
            repo,
            project,
            pull,
            commit_sha,
            base_url,
            token,
            token_env,
            revision,
            agent,
            dry_run,
        } => {
            run_import(BridgeImportRequest {
                provider,
                file,
                owner,
                repo,
                project,
                pull,
                commit_sha,
                base_url,
                token,
                token_env,
                revision,
                agent,
                dry_run,
                json: args.json,
            })
            .await
        }
    }
}

struct BridgeImportRequest {
    provider: String,
    file: Option<PathBuf>,
    owner: Option<String>,
    repo: Option<String>,
    project: Option<String>,
    pull: Option<String>,
    commit_sha: Option<String>,
    base_url: Option<String>,
    token: Option<String>,
    token_env: Option<String>,
    revision: String,
    agent: String,
    dry_run: bool,
    json: bool,
}

async fn run_import(request: BridgeImportRequest) -> anyhow::Result<()> {
    let provider = normalize_provider(&request.provider)?;
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let (revision_id, revision) = load_revision(&store, &request.revision)?;
    let (bytes, bundle) = load_bridge_bundle(&provider, &request).await?;

    let raw_ref_name = format!("bridges/{provider}/imports/{}", &sha256_hex(&bytes)[..16]);
    let raw_import = if request.dry_run {
        Some(BridgeObjectRef {
            object: sha256_hex(&bytes),
            ref_name: Some(raw_ref_name.clone()),
        })
    } else {
        let raw_id = store.store_object(&Object::Blob(Blob {
            data: bytes.clone(),
            media_type: Some("application/json".to_string()),
        }))?;
        store.set_ref(&raw_ref_name, &raw_id)?;
        Some(BridgeObjectRef {
            object: raw_id.to_hex(),
            ref_name: Some(raw_ref_name.clone()),
        })
    };

    let mapping = bridge_mapping_report(&bundle);
    let branch_policy = branch_policy_from_bundle(&provider, &bundle);
    let pr = pull_request_from_bundle(&provider, &bundle);
    let (intent_ref, change_ref) = if let Some(pr) = pr.as_ref() {
        import_pr_objects(
            &store,
            &provider,
            pr,
            branch_policy
                .as_ref()
                .map(|policy| policy.policy_id.as_str()),
            revision_id,
            request.dry_run,
        )?
    } else {
        (None, None)
    };
    let policy_ref = if let Some(policy) = branch_policy {
        Some(import_policy(&store, &provider, &policy, request.dry_run)?)
    } else {
        None
    };

    let evidence = evidence_from_bundle(&provider, &bundle, &revision_id);
    let evidence_added = evidence.len();
    let notes_imported = bridge_notes(&bundle).len();
    let capsule_ref = if evidence.is_empty() {
        None
    } else {
        Some(attach_bridge_evidence(
            &store,
            &provider,
            &request.agent,
            revision_id,
            &revision,
            evidence,
            request.dry_run,
        )?)
    };

    let report = BridgeImportReport {
        schema_version: 1,
        action: "bridge.import",
        provider,
        dry_run: request.dry_run,
        revision: revision_id.to_hex(),
        raw_import_ref: raw_import.and_then(|raw| raw.ref_name),
        intent: intent_ref,
        change: change_ref,
        capsule: capsule_ref,
        policy: policy_ref,
        evidence_added,
        notes_imported,
        mapping,
    };

    if request.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Imported {} bridge metadata for revision {}.",
            report.provider, report.revision
        );
        if let Some(intent) = &report.intent {
            println!("  Intent: {}", intent.object);
        }
        if let Some(change) = &report.change {
            println!("  Change: {}", change.object);
        }
        if let Some(capsule) = &report.capsule {
            println!("  Capsule: {}", capsule.object);
        }
        if let Some(policy) = &report.policy {
            println!("  Policy: {}", policy.object);
        }
        println!("  Evidence added: {}", report.evidence_added);
        println!("  Notes imported: {}", report.notes_imported);
        if report.mapping.branch_protection_present {
            println!(
                "  Branch protection: required_checks={}, required_reviewers={}, approvals={}",
                report.mapping.required_check_count,
                report.mapping.required_reviewer_count,
                report
                    .mapping
                    .required_approving_review_count
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "n/a".to_string())
            );
        }
    }

    Ok(())
}

async fn load_bridge_bundle(
    provider: &str,
    request: &BridgeImportRequest,
) -> anyhow::Result<(Vec<u8>, serde_json::Value)> {
    if let Some(file) = &request.file {
        if live_fetch_requested(request) {
            anyhow::bail!("pass either --file or live provider selectors, not both");
        }
        let bytes = std::fs::read(file)?;
        let bundle: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|err| anyhow::anyhow!("bridge file must be JSON: {err}"))?;
        return Ok((bytes, bundle));
    }

    let bundle = fetch_live_bundle(provider, request).await?;
    let bytes = serde_json::to_vec_pretty(&bundle)?;
    Ok((bytes, bundle))
}

fn live_fetch_requested(request: &BridgeImportRequest) -> bool {
    request.owner.is_some()
        || request.repo.is_some()
        || request.project.is_some()
        || request.pull.is_some()
        || request.commit_sha.is_some()
        || request.base_url.is_some()
        || request.token.is_some()
        || request.token_env.is_some()
}

async fn fetch_live_bundle(
    provider: &str,
    request: &BridgeImportRequest,
) -> anyhow::Result<serde_json::Value> {
    match provider {
        "github" => fetch_github_bundle(request).await,
        "gitlab" => fetch_gitlab_bundle(request).await,
        _ => anyhow::bail!("unsupported bridge provider: {provider}"),
    }
}

async fn fetch_github_bundle(request: &BridgeImportRequest) -> anyhow::Result<serde_json::Value> {
    let owner = required_arg(request.owner.as_deref(), "--owner")?;
    let repo = required_arg(request.repo.as_deref(), "--repo")?;
    let pull = required_arg(request.pull.as_deref(), "--pull")?;
    let base_url = request
        .base_url
        .as_deref()
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let client = provider_client(resolve_token(request)?)?;

    let pr_url = format!("{base_url}/repos/{owner}/{repo}/pulls/{pull}");
    let pull_request = fetch_required_json(&client, &pr_url).await?;
    let sha = request
        .commit_sha
        .clone()
        .or_else(|| string_field(&pull_request, &["head.sha"]))
        .ok_or_else(|| {
            anyhow::anyhow!("GitHub pull response did not include head.sha; pass --commit-sha")
        })?;
    let base_branch =
        string_field(&pull_request, &["base.ref"]).unwrap_or_else(|| "main".to_string());

    let checks_url = format!("{base_url}/repos/{owner}/{repo}/commits/{sha}/check-runs");
    let statuses_url = format!("{base_url}/repos/{owner}/{repo}/commits/{sha}/statuses");
    let reviews_url = format!("{base_url}/repos/{owner}/{repo}/pulls/{pull}/reviews");
    let protection_url =
        format!("{base_url}/repos/{owner}/{repo}/branches/{base_branch}/protection");

    let checks = fetch_optional_json(&client, &checks_url)
        .await
        .and_then(|value| {
            value
                .get("check_runs")
                .and_then(|items| items.as_array())
                .cloned()
        })
        .unwrap_or_default();
    let statuses = fetch_optional_json(&client, &statuses_url)
        .await
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let reviews = fetch_optional_json(&client, &reviews_url)
        .await
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let protection = fetch_optional_json(&client, &protection_url).await;

    Ok(serde_json::json!({
        "source": {
            "provider": "github",
            "owner": owner,
            "repo": repo,
            "pull": pull,
            "commit_sha": sha,
            "base_url": base_url,
        },
        "pull_request": pull_request,
        "checks": checks,
        "statuses": statuses,
        "reviews": reviews,
        "branch_protection": protection,
    }))
}

async fn fetch_gitlab_bundle(request: &BridgeImportRequest) -> anyhow::Result<serde_json::Value> {
    let project = request
        .project
        .clone()
        .or_else(
            || match (request.owner.as_deref(), request.repo.as_deref()) {
                (Some(owner), Some(repo)) => Some(format!("{owner}/{repo}")),
                _ => None,
            },
        )
        .ok_or_else(|| {
            anyhow::anyhow!("--project or --owner plus --repo is required for GitLab live fetch")
        })?;
    let pull = required_arg(request.pull.as_deref(), "--pull")?;
    let project_encoded = urlencoding::encode(&project);
    let base_url = request
        .base_url
        .as_deref()
        .unwrap_or("https://gitlab.com/api/v4")
        .trim_end_matches('/');
    let client = provider_client(resolve_token(request)?)?;

    let mr_url = format!("{base_url}/projects/{project_encoded}/merge_requests/{pull}");
    let merge_request = fetch_required_json(&client, &mr_url).await?;
    let sha = request
        .commit_sha
        .clone()
        .or_else(|| string_field(&merge_request, &["sha", "diff_refs.head_sha"]))
        .ok_or_else(|| {
            anyhow::anyhow!("GitLab merge request response did not include sha; pass --commit-sha")
        })?;
    let target_branch =
        string_field(&merge_request, &["target_branch"]).unwrap_or_else(|| "main".to_string());
    let target_branch_encoded = urlencoding::encode(&target_branch);

    let statuses_url =
        format!("{base_url}/projects/{project_encoded}/repository/commits/{sha}/statuses");
    let approvals_url =
        format!("{base_url}/projects/{project_encoded}/merge_requests/{pull}/approvals");
    let notes_url = format!("{base_url}/projects/{project_encoded}/merge_requests/{pull}/notes");
    let protection_url =
        format!("{base_url}/projects/{project_encoded}/protected_branches/{target_branch_encoded}");

    let statuses = fetch_optional_json(&client, &statuses_url)
        .await
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let approvals = fetch_optional_json(&client, &approvals_url).await;
    let notes = fetch_optional_json(&client, &notes_url)
        .await
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let protection = fetch_optional_json(&client, &protection_url).await;
    let reviews = approvals
        .as_ref()
        .and_then(|value| value.get("approved_by"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "state": "approved",
                        "user": item.get("user").cloned().unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(serde_json::json!({
        "source": {
            "provider": "gitlab",
            "project": project,
            "pull": pull,
            "commit_sha": sha,
            "base_url": base_url,
        },
        "merge_request": merge_request,
        "statuses": statuses,
        "reviews": reviews,
        "git_notes": notes,
        "protected_branch": protection,
    }))
}

fn provider_client(token: Option<String>) -> anyhow::Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("claw-vcs-bridge"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if let Some(token) = token {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|err| anyhow::anyhow!("invalid provider token header: {err}"))?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .build()?)
}

fn resolve_token(request: &BridgeImportRequest) -> anyhow::Result<Option<String>> {
    if let Some(token) = &request.token {
        return Ok(Some(token.clone()));
    }
    if let Some(var) = &request.token_env {
        let token = std::env::var(var)
            .map_err(|_| anyhow::anyhow!("token environment variable '{var}' is not set"))?;
        if token.trim().is_empty() {
            anyhow::bail!("token environment variable '{var}' is empty");
        }
        return Ok(Some(token));
    }
    Ok(None)
}

async fn fetch_required_json(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<serde_json::Value> {
    let response = client.get(url).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("provider request failed ({status}) for {url}: {text}");
    }
    serde_json::from_str(&text)
        .map_err(|err| anyhow::anyhow!("provider response from {url} was not JSON: {err}"))
}

async fn fetch_optional_json(client: &reqwest::Client, url: &str) -> Option<serde_json::Value> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<serde_json::Value>().await.ok()
}

fn required_arg(value: Option<&str>, flag: &str) -> anyhow::Result<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("{flag} is required for live bridge import"))
}

fn normalize_provider(provider: &str) -> anyhow::Result<String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "github" | "gh" => Ok("github".to_string()),
        "gitlab" | "gl" => Ok("gitlab".to_string()),
        other => anyhow::bail!("unsupported bridge provider: {other}"),
    }
}

fn import_pr_objects(
    store: &ClawStore,
    provider: &str,
    pr: &BridgePr,
    policy_id: Option<&str>,
    revision_id: ObjectId,
    dry_run: bool,
) -> anyhow::Result<(Option<BridgeObjectRef>, Option<BridgeObjectRef>)> {
    let now = current_time_ms();
    let intent_id = IntentId::new();
    let change_id = ChangeId::new();
    let mut links = Vec::new();
    if let Some(url) = pr.url.clone() {
        links.push(url);
    }
    let mut constraints = vec![format!("Imported from {provider} {}", pr.key)];
    if let Some(source) = &pr.source_branch {
        constraints.push(format!("source branch: {source}"));
    }
    if let Some(target) = &pr.target_branch {
        constraints.push(format!("target branch: {target}"));
    }
    let policy_refs = policy_id
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let intent = Intent {
        id: intent_id,
        title: pr.title.clone(),
        goal: if pr.body.is_empty() {
            format!("Preserve imported {provider} {}", pr.key)
        } else {
            pr.body.clone()
        },
        constraints,
        acceptance_tests: vec![],
        links,
        policy_refs,
        agents: vec![provider.to_string()],
        change_ids: vec![change_id.to_string()],
        depends_on: vec![],
        supersedes: vec![],
        status: intent_status_from_provider(&pr.state),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let change = Change {
        id: change_id,
        intent_id,
        head_revision: Some(revision_id),
        workstream_id: Some(format!("{provider}:{}", pr.key)),
        status: change_status_from_provider(&pr.state),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let intent_ref = format!(
        "bridges/{provider}/pulls/{}/intent",
        sanitize_ref_part(&pr.key)
    );
    let change_ref = format!(
        "bridges/{provider}/pulls/{}/change",
        sanitize_ref_part(&pr.key)
    );
    if dry_run {
        return Ok((
            Some(BridgeObjectRef {
                object: intent.id.to_string(),
                ref_name: Some(intent_ref),
            }),
            Some(BridgeObjectRef {
                object: change.id.to_string(),
                ref_name: Some(change_ref),
            }),
        ));
    }

    let intent_obj = store.store_object(&Object::Intent(intent.clone()))?;
    store.set_ref(&format!("intents/{}", intent.id), &intent_obj)?;
    store.set_ref(&intent_ref, &intent_obj)?;
    let change_obj = store.store_object(&Object::Change(change.clone()))?;
    store.set_ref(&format!("changes/{}", change.id), &change_obj)?;
    store.set_ref(&change_ref, &change_obj)?;
    Ok((
        Some(BridgeObjectRef {
            object: intent_obj.to_hex(),
            ref_name: Some(intent_ref),
        }),
        Some(BridgeObjectRef {
            object: change_obj.to_hex(),
            ref_name: Some(change_ref),
        }),
    ))
}

fn import_policy(
    store: &ClawStore,
    provider: &str,
    policy: &Policy,
    dry_run: bool,
) -> anyhow::Result<BridgeObjectRef> {
    let ref_name = format!("policies/{}", policy.policy_id);
    if dry_run {
        return Ok(BridgeObjectRef {
            object: policy.policy_id.clone(),
            ref_name: Some(ref_name),
        });
    }
    let id = store.store_object(&Object::Policy(policy.clone()))?;
    store.set_ref(&ref_name, &id)?;
    store.set_ref(
        &format!("bridges/{provider}/policies/{}", policy.policy_id),
        &id,
    )?;
    Ok(BridgeObjectRef {
        object: id.to_hex(),
        ref_name: Some(ref_name),
    })
}

fn attach_bridge_evidence(
    store: &ClawStore,
    provider: &str,
    agent: &str,
    revision_id: ObjectId,
    revision: &claw_core::types::Revision,
    evidence: Vec<Evidence>,
    dry_run: bool,
) -> anyhow::Result<BridgeObjectRef> {
    let ref_name = format!("capsules/by-revision/{}", revision_id.to_hex());
    if dry_run {
        return Ok(BridgeObjectRef {
            object: revision_id.to_hex(),
            ref_name: Some(ref_name),
        });
    }

    let registered_agent = ensure_registered_signing_agent(store, agent)?;
    let keypair = keypair_for_agent(agent, &registered_agent)?;
    let mut capsule = match load_default_capsule(store, &revision_id, revision) {
        Ok((_id, capsule)) => capsule,
        Err(_) => build_capsule(
            &revision_id,
            CapsulePublic {
                agent_id: registered_agent.agent_id.clone(),
                agent_version: registered_agent.agent_version.clone(),
                toolchain_digest: None,
                env_fingerprint: None,
                evidence: vec![],
            },
            None,
            None,
            &keypair,
        )?,
    };
    capsule.public_fields.evidence.extend(evidence);
    capsule.public_fields.agent_id = registered_agent.agent_id.clone();
    capsule.public_fields.agent_version = registered_agent.agent_version.clone();
    capsule.key_id = Some(registered_agent.public_key.clone());
    capsule.signatures.clear();
    append_capsule_signature(&mut capsule, &keypair)?;

    let capsule_id = store.store_object(&Object::Capsule(capsule.clone()))?;
    store.set_ref(&format!("capsules/{}", revision_id.to_hex()), &capsule_id)?;
    store.set_ref(&ref_name, &capsule_id)?;
    store.set_ref(
        &format!("bridges/{provider}/capsules/{}", revision_id.to_hex()),
        &capsule_id,
    )?;
    Ok(BridgeObjectRef {
        object: capsule_id.to_hex(),
        ref_name: Some(ref_name),
    })
}

fn pull_request_from_bundle(provider: &str, bundle: &serde_json::Value) -> Option<BridgePr> {
    let pr = first_object(
        bundle,
        &[
            "pull_request",
            "pullRequest",
            "merge_request",
            "mergeRequest",
        ],
    )
    .unwrap_or(bundle);
    let number = string_field(pr, &["number", "iid", "id"])?;
    let title = string_field(pr, &["title"]).unwrap_or_else(|| format!("{provider} {number}"));
    let body = string_field(pr, &["body", "description"]).unwrap_or_default();
    let state = string_field(pr, &["state", "merge_status", "status"]).unwrap_or_default();
    Some(BridgePr {
        key: number,
        title,
        body,
        state,
        url: string_field(pr, &["html_url", "web_url", "url"]),
        source_branch: string_field(pr, &["head.ref", "source_branch", "sourceBranch"]),
        target_branch: string_field(pr, &["base.ref", "target_branch", "targetBranch"]),
    })
}

fn branch_policy_from_bundle(provider: &str, bundle: &serde_json::Value) -> Option<Policy> {
    let protection = branch_protection_from_bundle(bundle)?;
    let branch =
        string_field(protection, &["name", "branch", "pattern"]).unwrap_or("default".into());
    let required_checks = required_checks(protection);
    let required_reviewers = string_array_field(protection, &["required_reviewers", "reviewers"]);
    if required_checks.is_empty() && required_reviewers.is_empty() {
        return None;
    }
    Some(Policy {
        policy_id: format!("{provider}-branch-{}", sanitize_policy_part(&branch)),
        required_checks,
        required_reviewers,
        sensitive_paths: vec![],
        quarantine_lane: false,
        min_trust_score: None,
        visibility: Visibility::Public,
        authorized_recipients: vec![],
        revoked_recipients: vec![],
        evidence_policy: EvidencePolicy {
            require_fresh_evidence: false,
            ..EvidencePolicy::default()
        },
    })
}

fn bridge_mapping_report(bundle: &serde_json::Value) -> BridgeMappingReport {
    let protection = branch_protection_from_bundle(bundle);
    let required_checks = protection.map(required_checks).unwrap_or_default();
    let required_reviewers = protection
        .map(|protection| string_array_field(protection, &["required_reviewers", "reviewers"]))
        .unwrap_or_default();
    let review_rules = protection
        .map(branch_protection_review_rules)
        .unwrap_or_default();

    BridgeMappingReport {
        pull_request_present: pull_request_from_bundle("provider", bundle).is_some(),
        check_count: array_field(bundle, &["checks", "check_runs", "checkRuns"]).len(),
        status_count: array_field(bundle, &["statuses", "commit_statuses", "commitStatuses"]).len(),
        review_count: array_field(bundle, &["reviews", "review_decisions", "reviewDecisions"])
            .len(),
        note_count: bridge_notes(bundle).len(),
        branch_protection_present: protection.is_some(),
        required_check_count: required_checks.len(),
        required_reviewer_count: required_reviewers.len(),
        required_approving_review_count: review_rules.required_approving_review_count,
        require_code_owner_reviews: review_rules.require_code_owner_reviews,
        dismiss_stale_reviews: review_rules.dismiss_stale_reviews,
        require_last_push_approval: review_rules.require_last_push_approval,
    }
}

fn branch_protection_from_bundle(bundle: &serde_json::Value) -> Option<&serde_json::Value> {
    first_object(
        bundle,
        &[
            "branch_protection",
            "branchProtection",
            "protected_branch",
            "protectedBranch",
        ],
    )
}

fn branch_protection_review_rules(protection: &serde_json::Value) -> BranchProtectionReviewRules {
    BranchProtectionReviewRules {
        required_approving_review_count: protection
            .pointer("/required_pull_request_reviews/required_approving_review_count")
            .and_then(serde_json::Value::as_u64)
            .or_else(|| {
                protection
                    .get("required_approving_review_count")
                    .and_then(serde_json::Value::as_u64)
            })
            .or_else(|| {
                protection
                    .get("approvals_required")
                    .and_then(serde_json::Value::as_u64)
            }),
        require_code_owner_reviews: protection
            .pointer("/required_pull_request_reviews/require_code_owner_reviews")
            .and_then(serde_json::Value::as_bool)
            .or_else(|| {
                protection
                    .get("require_code_owner_reviews")
                    .and_then(serde_json::Value::as_bool)
            })
            .unwrap_or(false),
        dismiss_stale_reviews: protection
            .pointer("/required_pull_request_reviews/dismiss_stale_reviews")
            .and_then(serde_json::Value::as_bool)
            .or_else(|| {
                protection
                    .get("dismiss_stale_reviews")
                    .and_then(serde_json::Value::as_bool)
            })
            .unwrap_or(false),
        require_last_push_approval: protection
            .pointer("/required_pull_request_reviews/require_last_push_approval")
            .and_then(serde_json::Value::as_bool)
            .or_else(|| {
                protection
                    .get("require_last_push_approval")
                    .and_then(serde_json::Value::as_bool)
            })
            .unwrap_or(false),
    }
}

fn evidence_from_bundle(
    provider: &str,
    bundle: &serde_json::Value,
    revision_id: &ObjectId,
) -> Vec<Evidence> {
    let now = current_time_ms();
    let mut evidence = Vec::new();
    for item in array_field(bundle, &["checks", "check_runs", "checkRuns"]) {
        let name = string_field(item, &["name", "context"]).unwrap_or_else(|| "check".to_string());
        let raw = string_field(item, &["conclusion", "state", "status"]).unwrap_or_default();
        evidence.push(provider_evidence(
            &format!("check:{name}"),
            provider_status(&raw),
            provider,
            revision_id,
            now,
            string_field(item, &["details_url", "html_url", "web_url", "url"]),
            Some(format!("{provider} check state: {raw}")),
        ));
    }
    for item in array_field(bundle, &["statuses", "commit_statuses", "commitStatuses"]) {
        let name = string_field(item, &["context", "name"]).unwrap_or_else(|| "status".to_string());
        let raw = string_field(item, &["state", "status"]).unwrap_or_default();
        evidence.push(provider_evidence(
            &format!("status:{name}"),
            provider_status(&raw),
            provider,
            revision_id,
            now,
            string_field(item, &["target_url", "details_url", "url"]),
            Some(format!("{provider} status state: {raw}")),
        ));
    }
    for item in array_field(bundle, &["reviews", "review_decisions", "reviewDecisions"]) {
        let reviewer = string_field(
            item,
            &[
                "user.login",
                "author.username",
                "username",
                "reviewer",
                "user",
            ],
        )
        .unwrap_or_else(|| "reviewer".to_string());
        let raw = string_field(item, &["state", "status"]).unwrap_or_default();
        evidence.push(provider_evidence(
            &format!("review:{reviewer}"),
            review_status(&raw),
            provider,
            revision_id,
            now,
            string_field(item, &["html_url", "web_url", "url"]),
            Some(format!("{provider} review state: {raw}")),
        ));
    }
    for note in bridge_notes(bundle) {
        evidence.push(provider_evidence(
            "git-note",
            "pass",
            provider,
            revision_id,
            now,
            None,
            Some(note),
        ));
    }
    evidence
}

fn provider_evidence(
    name: &str,
    status: &str,
    provider: &str,
    revision_id: &ObjectId,
    now: u64,
    url: Option<String>,
    summary: Option<String>,
) -> Evidence {
    Evidence {
        name: name.to_string(),
        status: status.to_string(),
        duration_ms: 0,
        artifact_refs: url.into_iter().collect(),
        summary,
        revision_id: Some(*revision_id),
        command: None,
        exit_code: Some(if status == "pass" { 0 } else { 1 }),
        started_at_ms: Some(now),
        ended_at_ms: Some(now),
        environment_digest: None,
        runner_identity: Some(format!("{provider}-bridge")),
        log_digest: None,
        artifact_digest: None,
        expires_at_ms: None,
        trust_domain: Some(format!("{provider}-hosted")),
        signature: None,
    }
}

fn required_checks(protection: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(
        string_array_field(
            protection,
            &[
                "required_status_checks.contexts",
                "requiredStatusChecks.contexts",
                "required_status_checks",
            ],
        )
        .into_iter()
        .map(|name| format!("status:{name}")),
    );
    for item in array_field(
        protection,
        &[
            "required_status_checks.checks",
            "requiredStatusChecks.checks",
        ],
    ) {
        if let Some(name) = string_field(item, &["context", "name"]) {
            out.push(format!("check:{name}"));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn bridge_notes(bundle: &serde_json::Value) -> Vec<String> {
    array_field(bundle, &["git_notes", "gitNotes", "notes"])
        .into_iter()
        .filter_map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| string_field(value, &["body", "note", "message", "text"]))
        })
        .collect()
}

fn provider_status(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "success" | "successful" | "passed" | "pass" | "completed" => "pass",
        "neutral" | "skipped" | "pending" | "queued" | "in_progress" | "running" => "pending",
        _ => "fail",
    }
}

fn review_status(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "approved" | "approval" | "approve" => "pass",
        "commented" | "pending" => "pending",
        _ => "fail",
    }
}

fn intent_status_from_provider(value: &str) -> IntentStatus {
    match value.to_ascii_lowercase().as_str() {
        "closed" | "merged" | "merged_result" => IntentStatus::Done,
        "blocked" => IntentStatus::Blocked,
        _ => IntentStatus::Open,
    }
}

fn change_status_from_provider(value: &str) -> ChangeStatus {
    match value.to_ascii_lowercase().as_str() {
        "closed" | "merged" | "merged_result" => ChangeStatus::Integrated,
        "abandoned" => ChangeStatus::Abandoned,
        _ => ChangeStatus::Ready,
    }
}

fn first_object<'a>(value: &'a serde_json::Value, paths: &[&str]) -> Option<&'a serde_json::Value> {
    paths.iter().find_map(|path| nested(value, path))
}

fn array_field<'a>(value: &'a serde_json::Value, paths: &[&str]) -> Vec<&'a serde_json::Value> {
    paths
        .iter()
        .find_map(|path| nested(value, path).and_then(|value| value.as_array()))
        .map(|array| array.iter().collect())
        .unwrap_or_default()
}

fn string_array_field(value: &serde_json::Value, paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .find_map(|path| nested(value, path).and_then(|value| value.as_array()))
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn string_field(value: &serde_json::Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        nested(value, path).and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_i64().map(|n| n.to_string()))
                .or_else(|| value.as_u64().map(|n| n.to_string()))
        })
    })
}

fn nested<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn sanitize_ref_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn sanitize_policy_part(value: &str) -> String {
    let sanitized = sanitize_ref_part(value).to_ascii_lowercase();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::json;

    use super::{
        branch_policy_from_bundle, bridge_mapping_report, evidence_from_bundle,
        pull_request_from_bundle, BridgeArgs, BridgeCommand,
    };

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: BridgeArgs,
    }

    #[test]
    fn parses_bridge_import() {
        let cli = TestCli::parse_from([
            "claw",
            "--json",
            "import",
            "--provider",
            "github",
            "--file",
            "bridge.json",
            "--revision",
            "heads/main",
            "--dry-run",
        ]);
        assert!(cli.args.json);
        match cli.args.command {
            BridgeCommand::Import {
                provider, dry_run, ..
            } => {
                assert_eq!(provider, "github");
                assert!(dry_run);
            }
        }
    }

    #[test]
    fn parses_live_github_bridge_import() {
        let cli = TestCli::parse_from([
            "claw",
            "import",
            "--provider",
            "github",
            "--owner",
            "acme",
            "--repo",
            "widget",
            "--pull",
            "42",
            "--commit-sha",
            "abc123",
            "--token-env",
            "GITHUB_TOKEN",
            "--revision",
            "heads/main",
        ]);
        match cli.args.command {
            BridgeCommand::Import {
                provider,
                file,
                owner,
                repo,
                pull,
                commit_sha,
                token_env,
                ..
            } => {
                assert_eq!(provider, "github");
                assert!(file.is_none());
                assert_eq!(owner.as_deref(), Some("acme"));
                assert_eq!(repo.as_deref(), Some("widget"));
                assert_eq!(pull.as_deref(), Some("42"));
                assert_eq!(commit_sha.as_deref(), Some("abc123"));
                assert_eq!(token_env.as_deref(), Some("GITHUB_TOKEN"));
            }
        }
    }

    #[test]
    fn maps_github_bundle_to_pr_policy_and_evidence() {
        let bundle = json!({
            "pull_request": {
                "number": 42,
                "title": "Add thing",
                "body": "Useful context",
                "state": "open",
                "html_url": "https://github.test/repo/pull/42",
                "head": {"ref": "feature"},
                "base": {"ref": "main"}
            },
            "checks": [{"name": "test", "conclusion": "success"}],
            "statuses": [{"context": "lint", "state": "failure"}],
            "reviews": [{"user": {"login": "alice"}, "state": "APPROVED"}],
            "branch_protection": {
                "name": "main",
                "required_status_checks": {
                    "contexts": ["lint"],
                    "checks": [{"context": "test"}]
                },
                "required_pull_request_reviews": {
                    "required_approving_review_count": 2,
                    "require_code_owner_reviews": true,
                    "dismiss_stale_reviews": true,
                    "require_last_push_approval": true
                }
            },
            "git_notes": [{"body": "legacy provenance note"}]
        });
        let pr = pull_request_from_bundle("github", &bundle).unwrap();
        assert_eq!(pr.key, "42");
        assert_eq!(pr.source_branch.as_deref(), Some("feature"));
        let policy = branch_policy_from_bundle("github", &bundle).unwrap();
        assert_eq!(policy.policy_id, "github-branch-main");
        assert!(policy.required_checks.contains(&"check:test".to_string()));
        assert!(policy.required_checks.contains(&"status:lint".to_string()));
        let mapping = bridge_mapping_report(&bundle);
        assert_eq!(mapping.required_check_count, 2);
        assert_eq!(mapping.required_approving_review_count, Some(2));
        assert!(mapping.require_code_owner_reviews);
        assert!(mapping.dismiss_stale_reviews);
        assert!(mapping.require_last_push_approval);
        let revision =
            claw_core::hash::content_hash(claw_core::object::TypeTag::Revision, b"bridge");
        let evidence = evidence_from_bundle("github", &bundle, &revision);
        assert!(evidence
            .iter()
            .any(|item| item.name == "check:test" && item.status == "pass"));
        assert!(evidence
            .iter()
            .any(|item| item.name == "status:lint" && item.status == "fail"));
        assert!(evidence
            .iter()
            .any(|item| item.name == "review:alice" && item.status == "pass"));
        assert!(evidence.iter().any(|item| item.name == "git-note"));
    }
}
