use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_policy::{evaluator::evaluate_policy, PolicyContext};
use claw_store::{ClawStore, HeadState};
use claw_sync::client::{RetryPolicy, SyncClient};
use claw_sync::compat::{compatibility_report, CompatibilityLevel};
use claw_sync::negotiation::ordered_reachable_objects;
use claw_sync::proto::sync::{HelloResponse, PartialCloneFilter};
use claw_sync::protocol::{
    negotiated_protocol_version, server_capabilities, CAP_POLICY_AWARE_PUSH, SYNC_PROTOCOL_VERSION,
};
use claw_sync::transport::{
    GrpcTlsConfig, RefUpdateContext, RefUpdatePolicyCheck, RemoteTransportConfig,
};

use crate::auth_store;
use crate::config::{self, find_repo_root};
use crate::worktree;

use super::object_refs::{
    current_time_ms, derive_capsule_trust_score, load_default_capsule, load_policy,
};
use super::{remote, RuntimeOptions};

const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

fn resolve_token_profiles(
    token_profile: Option<&str>,
    runtime_profile: &str,
    repo_default_profile: &str,
) -> Vec<String> {
    let explicit_profile = token_profile
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(profile) = explicit_profile {
        return vec![profile.to_string()];
    }

    let mut profiles = Vec::new();
    let runtime_profile = runtime_profile.trim();
    if !runtime_profile.is_empty() {
        profiles.push(runtime_profile.to_string());
    }

    let repo_default_profile = repo_default_profile.trim();
    if !repo_default_profile.is_empty() && !profiles.iter().any(|p| p == repo_default_profile) {
        profiles.push(repo_default_profile.to_string());
    }

    if profiles.is_empty() {
        profiles.push("default".to_string());
    }

    profiles
}

fn require_access_token(
    token_profile: Option<&str>,
    runtime_profile: &str,
    repo_default_profile: &str,
) -> anyhow::Result<String> {
    let candidates = resolve_token_profiles(token_profile, runtime_profile, repo_default_profile);
    for profile_name in &candidates {
        if let Some(token) = auth_store::try_resolve_access_token(Some(profile_name))? {
            return Ok(token);
        }
    }

    let suggested_profile = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    anyhow::bail!(
        "no token found for profiles [{}]; run `claw auth login --profile {}`",
        candidates.join(", "),
        suggested_profile
    );
}

#[derive(Args)]
pub struct SyncArgs {
    /// Trust this PEM CA certificate when connecting to gRPC remotes over TLS
    #[arg(long)]
    tls_ca_cert: Option<PathBuf>,
    /// Override the TLS server name used for gRPC certificate verification
    #[arg(long)]
    tls_domain: Option<String>,
    /// PEM client certificate for mutual TLS with gRPC remotes
    #[arg(long)]
    client_cert: Option<PathBuf>,
    /// PEM client private key for mutual TLS with gRPC remotes
    #[arg(long)]
    client_key: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<SyncCommand>,
    /// Remote name or address (compatibility form: `claw sync <remote>`)
    remote: Option<String>,
}

#[derive(Subcommand)]
enum SyncCommand {
    /// Push objects to remote
    Push {
        /// Remote name or address (e.g., origin or http://localhost:50051)
        #[arg(short, long, default_value = "origin")]
        remote: String,
        /// Ref to push
        #[arg(short = 'b', long, default_value = "heads/main")]
        ref_name: String,
        /// Output a machine-readable JSON receipt
        #[arg(long)]
        json: bool,
        /// Force non-fast-forward push
        #[arg(long)]
        force: bool,
        /// Preview objects and ref update without uploading or mutating remote refs
        #[arg(long)]
        dry_run: bool,
        /// Evaluate this repository policy before uploading or updating the remote ref. Repeatable.
        #[arg(long = "policy")]
        policies: Vec<String>,
    },
    /// Pull objects from remote
    Pull {
        /// Remote name or address
        #[arg(short, long, default_value = "origin")]
        remote: String,
        /// Ref to pull
        #[arg(short = 'b', long, default_value = "heads/main")]
        ref_name: String,
        /// Force non-fast-forward update
        #[arg(long)]
        force: bool,
        /// Output a machine-readable JSON receipt
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FetchFilterArgs,
    },
    /// Clone a remote repository
    Clone {
        /// Remote address
        remote: String,
        /// Transport kind (grpc|clawlab)
        #[arg(long, default_value = "grpc")]
        kind: String,
        /// Repository slug for clawlab remotes
        #[arg(long)]
        repo: Option<String>,
        /// Auth profile for clawlab remotes
        #[arg(long)]
        token_profile: Option<String>,
        /// Local path
        #[arg(default_value = ".")]
        path: String,
        /// Output a machine-readable JSON receipt
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        filter: FetchFilterArgs,
    },
}

#[derive(Args, Clone, Debug, Default)]
struct FetchFilterArgs {
    /// Include objects associated with this intent ID. Repeatable.
    #[arg(long = "intent")]
    intent_ids: Vec<String>,
    /// Include patches under this repository path prefix. Repeatable.
    #[arg(long = "path-prefix")]
    path_prefixes: Vec<String>,
    /// Include patches using this codec ID. Repeatable.
    #[arg(long = "codec")]
    codec_ids: Vec<String>,
    /// Include revisions created at or after this Unix epoch millisecond.
    #[arg(long = "time-start-ms")]
    time_start_ms: Option<u64>,
    /// Include revisions created at or before this Unix epoch millisecond.
    #[arg(long = "time-end-ms")]
    time_end_ms: Option<u64>,
    /// Filter capsules by visibility: public|private|restricted.
    #[arg(long = "visibility")]
    capsule_visibility: Option<String>,
    /// Limit object traversal depth from requested refs.
    #[arg(long = "depth")]
    max_depth: Option<u32>,
    /// Stop streaming after this approximate byte budget.
    #[arg(long = "bytes", alias = "byte-budget")]
    max_bytes: Option<u64>,
}

fn resolve_command(args: SyncArgs) -> SyncCommand {
    match args.command {
        Some(command) => command,
        None => SyncCommand::Pull {
            remote: args.remote.unwrap_or_else(|| "origin".to_string()),
            ref_name: "heads/main".to_string(),
            force: false,
            json: false,
            filter: FetchFilterArgs::default(),
        },
    }
}

fn build_fetch_filter(args: &FetchFilterArgs) -> Option<PartialCloneFilter> {
    let empty = args.intent_ids.is_empty()
        && args.path_prefixes.is_empty()
        && args.codec_ids.is_empty()
        && args.time_start_ms.is_none()
        && args.time_end_ms.is_none()
        && args.capsule_visibility.is_none()
        && args.max_depth.is_none()
        && args.max_bytes.is_none();

    (!empty).then(|| PartialCloneFilter {
        intent_ids: args.intent_ids.clone(),
        path_prefixes: args.path_prefixes.clone(),
        time_range_start: args.time_start_ms.unwrap_or_default(),
        time_range_end: args.time_end_ms.unwrap_or_default(),
        codec_ids: args.codec_ids.clone(),
        capsule_visibility: args.capsule_visibility.clone().unwrap_or_default(),
        max_bytes: args.max_bytes.unwrap_or_default(),
        max_depth: args.max_depth.unwrap_or_default(),
    })
}

fn object_available(store: &ClawStore, id: &ObjectId) -> bool {
    store.load_object(id).is_ok()
}

async fn connect_from_remote(
    root: &Path,
    remote_arg: &str,
    runtime: &RuntimeOptions,
    grpc_tls: Option<GrpcTlsConfig>,
) -> anyhow::Result<SyncClient> {
    let resolved = remote::resolve_remote(root, remote_arg)?;
    let cfg = config::load_or_default_config(root)?;
    let repo_default_profile = config::default_profile(&cfg);
    let transport = match resolved {
        remote::ResolvedRemote::Grpc {
            addr,
            token_profile,
        } => {
            let bearer_token = token_profile
                .as_deref()
                .map(|profile| {
                    require_access_token(Some(profile), &runtime.profile, repo_default_profile)
                })
                .transpose()?;
            RemoteTransportConfig::Grpc {
                addr,
                bearer_token,
                tls: grpc_tls,
            }
        }
        remote::ResolvedRemote::ClawLab {
            base_url,
            repo,
            token_profile,
        } => {
            let token = require_access_token(
                token_profile.as_deref(),
                &runtime.profile,
                repo_default_profile,
            )?;
            RemoteTransportConfig::Http {
                base_url,
                repo,
                bearer_token: Some(token),
            }
        }
    };

    let retry_policy = RetryPolicy {
        idempotent_only: cfg.retries.idempotent_only,
        max_attempts: cfg.retries.max_attempts,
        base_backoff_ms: cfg.retries.base_backoff_ms,
        max_backoff_ms: cfg.retries.max_backoff_ms,
        jitter: cfg.retries.jitter,
    };

    let client = SyncClient::connect_with_transport_and_retry(transport, retry_policy).await?;
    Ok(client)
}

fn load_optional_pem(path: Option<&Path>) -> anyhow::Result<Option<Vec<u8>>> {
    path.map(std::fs::read).transpose().map_err(Into::into)
}

fn build_grpc_tls_config(args: &SyncArgs) -> anyhow::Result<Option<GrpcTlsConfig>> {
    if args.client_cert.is_some() != args.client_key.is_some() {
        anyhow::bail!("--client-cert and --client-key must be provided together");
    }

    if args.tls_ca_cert.is_none()
        && args.tls_domain.is_none()
        && args.client_cert.is_none()
        && args.client_key.is_none()
    {
        return Ok(None);
    }

    Ok(Some(GrpcTlsConfig {
        ca_cert_pem: load_optional_pem(args.tls_ca_cert.as_deref())?,
        client_cert_pem: load_optional_pem(args.client_cert.as_deref())?,
        client_key_pem: load_optional_pem(args.client_key.as_deref())?,
        domain_name: args.tls_domain.clone(),
    }))
}

fn check_remote_compatibility(remote: &str, hello: &HelloResponse) -> anyhow::Result<()> {
    let report = compatibility_report(CLI_VERSION, &hello.server_version);
    let protocol = negotiated_protocol_version(&hello.capabilities);
    if protocol != Some(SYNC_PROTOCOL_VERSION) {
        anyhow::bail!(
            "protocol negotiation failed for {remote}: expected {expected}, got capabilities [{caps}]",
            expected = SYNC_PROTOCOL_VERSION,
            caps = hello.capabilities.join(", "),
        );
    }

    match report.level {
        CompatibilityLevel::Full => Ok(()),
        CompatibilityLevel::Limited => {
            eprintln!(
                "compatibility check: limited support for {remote} (local {local}, remote {remote_ver}); N/N-1 compatibility applies, but prefer matching versions for best results",
                local = report.local,
                remote_ver = report.remote,
            );
            Ok(())
        }
        CompatibilityLevel::Unsupported => anyhow::bail!(
            "compatibility check failed for {remote}: local claw version '{local}' is incompatible with remote version '{remote_ver}'; use matching major version and at most one minor difference (N/N-1), or retry with --no-compat-check only after verifying the risk",
            local = report.local,
            remote_ver = report.remote,
        ),
    }
}

async fn maybe_check_compatibility(
    runtime: &RuntimeOptions,
    remote: &str,
    client: &mut SyncClient,
) -> anyhow::Result<()> {
    if runtime.compat_check {
        let hello = client.hello().await?;
        check_remote_compatibility(remote, &hello)?;
    }

    Ok(())
}

pub async fn run(args: SyncArgs, runtime: &RuntimeOptions) -> anyhow::Result<()> {
    let grpc_tls = build_grpc_tls_config(&args)?;
    match resolve_command(args) {
        SyncCommand::Push {
            remote,
            ref_name,
            json,
            force,
            dry_run,
            policies,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut client = connect_from_remote(&root, &remote, runtime, grpc_tls.clone()).await?;
            let hello = client.hello().await?;
            if runtime.compat_check {
                check_remote_compatibility(&remote, &hello)?;
            }

            let local_id = store
                .get_ref(&ref_name)?
                .ok_or_else(|| anyhow::anyhow!("ref not found: {ref_name}"))?;
            let policy_results = evaluate_push_policies(&store, &ref_name, local_id, &policies)?;
            let denied_policies = policy_results
                .iter()
                .filter(|result| !result.allowed)
                .collect::<Vec<_>>();
            if !denied_policies.is_empty() {
                if json {
                    print_push_policy_denial_json(
                        dry_run,
                        &remote,
                        &ref_name,
                        force,
                        local_id,
                        &policy_results,
                    )?;
                }
                let joined = denied_policies
                    .iter()
                    .map(|result| {
                        format!(
                            "{} ({})",
                            result.id,
                            result.reason.as_deref().unwrap_or("denied")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!("policy denied sync push for {ref_name}: {joined}");
            }

            let push_ids: Vec<ObjectId> = ordered_reachable_objects(&store, &[local_id]);

            let remote_refs = client.advertise_refs("").await?;
            let remote_old = remote_refs
                .iter()
                .find(|(name, _)| name == &ref_name)
                .map(|(_, id)| *id);

            let updates = vec![(ref_name.clone(), remote_old, local_id)];
            if dry_run {
                if json {
                    print_push_json(PushJson {
                        dry_run,
                        remote: &remote,
                        ref_name: &ref_name,
                        force,
                        local_id,
                        object_count: push_ids.len(),
                        remote_old,
                        upload_message: None,
                        ref_update_success: None,
                        ref_update_message: None,
                        policy_results: &policy_results,
                        remote_capabilities: &hello.capabilities,
                    })?;
                } else {
                    println!(
                        "Dry run: would push {} object(s) to {}.",
                        push_ids.len(),
                        remote
                    );
                    match remote_old {
                        Some(old) => println!("  Ref update: {ref_name} {old} -> {local_id}"),
                        None => println!("  Ref create: {ref_name} -> {local_id}"),
                    }
                    if force {
                        println!("  Force: true");
                    }
                    print_policy_gate_human(&policy_results);
                    println!("  Object upload skipped.");
                    println!("  Remote ref update skipped.");
                }
                return Ok(());
            }

            let resp = client.push_objects(&store, &push_ids).await?;
            if !json {
                println!("Push: {}", resp.message);
            }

            let ref_update_context = build_ref_update_context(&policy_results, &hello);
            let ref_resp = if ref_update_context.has_policy_checks() {
                client
                    .update_refs_with_context(&updates, force, ref_update_context)
                    .await?
            } else {
                client.update_refs(&updates, force).await?
            };

            if ref_resp.success {
                if json {
                    print_push_json(PushJson {
                        dry_run,
                        remote: &remote,
                        ref_name: &ref_name,
                        force,
                        local_id,
                        object_count: push_ids.len(),
                        remote_old,
                        upload_message: Some(resp.message.as_str()),
                        ref_update_success: Some(ref_resp.success),
                        ref_update_message: Some(ref_resp.message.as_str()),
                        policy_results: &policy_results,
                        remote_capabilities: &hello.capabilities,
                    })?;
                } else {
                    print_policy_gate_human(&policy_results);
                    println!("Pushed {} to {}", ref_name, remote);
                }
            } else {
                anyhow::bail!("ref update failed: {}", ref_resp.message);
            }
        }
        SyncCommand::Pull {
            remote,
            ref_name,
            force,
            json,
            filter,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut client = connect_from_remote(&root, &remote, runtime, grpc_tls.clone()).await?;
            maybe_check_compatibility(runtime, &remote, &mut client).await?;

            let remote_refs = client.advertise_refs("").await?;
            let remote_target = remote_refs
                .iter()
                .find(|(name, _)| name == &ref_name)
                .map(|(_, id)| *id);

            let remote_id = match remote_target {
                Some(id) => id,
                None => {
                    if json {
                        print_pull_json(PullJson {
                            remote: &remote,
                            ref_name: &ref_name,
                            force,
                            remote_ref_found: false,
                            remote_id: None,
                            fetched_count: 0,
                            target_available: false,
                            ref_update_skipped: true,
                            ref_update_old: None,
                            ref_update_success: Some(false),
                            ref_update_message: Some("remote ref not found"),
                            worktree_updated: false,
                            filter: &filter,
                        })?;
                    } else {
                        println!("Remote ref {ref_name} not found");
                    }
                    return Ok(());
                }
            };

            let local_id = store.get_ref(&ref_name)?;
            let have: Vec<ObjectId> = local_id.into_iter().collect();

            let fetched = client
                .fetch_objects_filtered(&store, &[remote_id], &have, build_fetch_filter(&filter))
                .await?;
            if !json {
                println!("Fetched {} objects", fetched.len());
            }

            if !object_available(&store, &remote_id) {
                if json {
                    print_pull_json(PullJson {
                        remote: &remote,
                        ref_name: &ref_name,
                        force,
                        remote_ref_found: true,
                        remote_id: Some(remote_id),
                        fetched_count: fetched.len(),
                        target_available: false,
                        ref_update_skipped: true,
                        ref_update_old: local_id,
                        ref_update_success: Some(false),
                        ref_update_message: Some("fetch filters did not include target revision"),
                        worktree_updated: false,
                        filter: &filter,
                    })?;
                } else {
                    println!(
                        "Ref update skipped: fetch filters did not include target {} for {}.",
                        remote_id, ref_name
                    );
                    println!(
                        "Run without filters, widen the filter, or inspect the partial object set before updating refs."
                    );
                }
                return Ok(());
            }

            if let Some(local) = store.get_ref(&ref_name)? {
                let is_ff = claw_sync::ancestry::is_ancestor(&store, &local, &remote_id);
                if !is_ff && !force {
                    anyhow::bail!(
                        "non-fast-forward update on {}; use --force to override",
                        ref_name
                    );
                }
            }

            let old = store.get_ref(&ref_name)?;
            store.update_ref_cas(&ref_name, old.as_ref(), &remote_id, "sync", "pull")?;
            if !json {
                println!("Updated {} to {}", ref_name, remote_id);
            }

            let mut worktree_updated = false;
            let head_state = store.read_head()?;
            if let HeadState::Symbolic {
                ref_name: ref head_ref,
            } = head_state
            {
                if *head_ref == ref_name {
                    let rev_obj = store.load_object(&remote_id)?;
                    if let Object::Revision(ref rev) = rev_obj {
                        if let Some(ref tree_id) = rev.tree {
                            worktree::materialize_tree(&store, tree_id, &root)?;
                            worktree_updated = true;
                            if !json {
                                println!("Working tree updated.");
                            }
                        }
                    }
                }
            }
            if json {
                print_pull_json(PullJson {
                    remote: &remote,
                    ref_name: &ref_name,
                    force,
                    remote_ref_found: true,
                    remote_id: Some(remote_id),
                    fetched_count: fetched.len(),
                    target_available: true,
                    ref_update_skipped: false,
                    ref_update_old: old,
                    ref_update_success: Some(true),
                    ref_update_message: Some("updated"),
                    worktree_updated,
                    filter: &filter,
                })?;
            }
        }
        SyncCommand::Clone {
            remote,
            kind,
            repo,
            token_profile,
            path,
            json,
            filter,
        } => {
            let root = std::path::Path::new(&path);
            let store = ClawStore::init(root)?;
            let cfg = config::load_or_default_config(root)?;
            let repo_default_profile = config::default_profile(&cfg);
            let retry_policy = RetryPolicy {
                idempotent_only: cfg.retries.idempotent_only,
                max_attempts: cfg.retries.max_attempts,
                base_backoff_ms: cfg.retries.base_backoff_ms,
                max_backoff_ms: cfg.retries.max_backoff_ms,
                jitter: cfg.retries.jitter,
            };
            let mut client = match kind.as_str() {
                "grpc" => {
                    let bearer_token = token_profile
                        .as_deref()
                        .map(|profile| {
                            require_access_token(
                                Some(profile),
                                &runtime.profile,
                                repo_default_profile,
                            )
                        })
                        .transpose()?;
                    SyncClient::connect_with_transport_and_retry(
                        RemoteTransportConfig::Grpc {
                            addr: remote.clone(),
                            bearer_token,
                            tls: grpc_tls.clone(),
                        },
                        retry_policy,
                    )
                    .await?
                }
                "clawlab" => {
                    let repo_slug = repo.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "--repo is required for --kind clawlab (example: acme/widgets)"
                        )
                    })?;
                    let token = require_access_token(
                        token_profile.as_deref(),
                        &runtime.profile,
                        repo_default_profile,
                    )?;
                    SyncClient::connect_with_transport_and_retry(
                        RemoteTransportConfig::Http {
                            base_url: remote.clone(),
                            repo: repo_slug,
                            bearer_token: Some(token),
                        },
                        retry_policy,
                    )
                    .await?
                }
                other => anyhow::bail!("unsupported --kind: {other} (expected grpc|clawlab)"),
            };

            let hello = client.hello().await?;
            if runtime.compat_check {
                check_remote_compatibility(&remote, &hello)?;
            }
            let remote_refs = client.advertise_refs("").await?;

            let want: Vec<_> = remote_refs.iter().map(|(_, id)| *id).collect();
            let fetched = client
                .fetch_objects_filtered(&store, &want, &[], build_fetch_filter(&filter))
                .await?;

            let mut installed_refs = 0usize;
            let mut skipped_refs = Vec::new();
            for (name, id) in &remote_refs {
                if object_available(&store, id) {
                    store.set_ref(name, id)?;
                    installed_refs += 1;
                } else {
                    skipped_refs.push((name.clone(), *id));
                }
            }

            store.write_head(&HeadState::Symbolic {
                ref_name: "heads/main".to_string(),
            })?;

            let main_id = store.get_ref("heads/main")?;
            let checkout_id = main_id.or_else(|| remote_refs.first().map(|(_, id)| *id));
            let checkout_target = checkout_id.map(|id| id.to_string());
            let mut worktree_updated = false;
            let mut checkout_skipped = false;
            if let Some(rev_id) = checkout_id {
                if let Ok(rev_obj) = store.load_object(&rev_id) {
                    if let Object::Revision(ref rev) = rev_obj {
                        if let Some(ref tree_id) = rev.tree {
                            worktree::materialize_tree(&store, tree_id, root)?;
                            worktree_updated = true;
                        }
                    }
                } else if !skipped_refs.is_empty() {
                    checkout_skipped = true;
                    if !json {
                        println!(
                            "Working tree checkout skipped: fetch filters did not include a fetched revision ref."
                        );
                    }
                }
            }

            let config_path = root.join(".claw").join("remotes.toml");
            let mut remotes = remote::load_remotes(&config_path)?;
            let origin_entry = match kind.as_str() {
                "grpc" => remote::RemoteEntry {
                    kind: Some("grpc".to_string()),
                    url: Some(remote.clone()),
                    token_profile: token_profile.clone(),
                    ..remote::RemoteEntry::default()
                },
                "clawlab" => remote::RemoteEntry {
                    kind: Some("clawlab".to_string()),
                    base_url: Some(remote.clone()),
                    repo: repo.clone(),
                    token_profile: token_profile.clone(),
                    ..remote::RemoteEntry::default()
                },
                _ => remote::RemoteEntry::default(),
            };
            remotes.remotes.insert("origin".to_string(), origin_entry);
            remote::save_remotes(&config_path, &remotes)?;

            if json {
                print_clone_json(CloneJson {
                    remote: &remote,
                    kind: &kind,
                    repo: repo.as_deref(),
                    path: &path,
                    fetched_count: fetched.len(),
                    remote_ref_count: remote_refs.len(),
                    installed_ref_count: installed_refs,
                    skipped_refs: &skipped_refs,
                    filter: &filter,
                    head: "heads/main",
                    checkout_target: checkout_target.as_deref(),
                    worktree_updated,
                    checkout_skipped,
                    remote_name: "origin",
                    remote_config_path: config_path.display().to_string(),
                })?;
            } else {
                println!(
                    "Cloned {} ({} objects, {} refs)",
                    remote,
                    fetched.len(),
                    installed_refs
                );
                if !skipped_refs.is_empty() {
                    println!(
                        "Filtered clone skipped {} ref(s) whose targets were not fetched.",
                        skipped_refs.len()
                    );
                }
            }
        }
    }
    Ok(())
}

struct PushJson<'a> {
    dry_run: bool,
    remote: &'a str,
    ref_name: &'a str,
    force: bool,
    local_id: ObjectId,
    object_count: usize,
    remote_old: Option<ObjectId>,
    upload_message: Option<&'a str>,
    ref_update_success: Option<bool>,
    ref_update_message: Option<&'a str>,
    policy_results: &'a [PushPolicyResult],
    remote_capabilities: &'a [String],
}

struct PullJson<'a> {
    remote: &'a str,
    ref_name: &'a str,
    force: bool,
    remote_ref_found: bool,
    remote_id: Option<ObjectId>,
    fetched_count: usize,
    target_available: bool,
    ref_update_skipped: bool,
    ref_update_old: Option<ObjectId>,
    ref_update_success: Option<bool>,
    ref_update_message: Option<&'a str>,
    worktree_updated: bool,
    filter: &'a FetchFilterArgs,
}

struct CloneJson<'a> {
    remote: &'a str,
    kind: &'a str,
    repo: Option<&'a str>,
    path: &'a str,
    fetched_count: usize,
    remote_ref_count: usize,
    installed_ref_count: usize,
    skipped_refs: &'a [(String, ObjectId)],
    filter: &'a FetchFilterArgs,
    head: &'a str,
    checkout_target: Option<&'a str>,
    worktree_updated: bool,
    checkout_skipped: bool,
    remote_name: &'a str,
    remote_config_path: String,
}

struct PushPolicyResult {
    id: String,
    ref_name: String,
    object: String,
    allowed: bool,
    reason: Option<String>,
}

fn print_clone_json(receipt: CloneJson<'_>) -> anyhow::Result<()> {
    let skipped_refs: Vec<_> = receipt
        .skipped_refs
        .iter()
        .map(|(name, id)| {
            serde_json::json!({
                "name": name,
                "target": id.to_string(),
            })
        })
        .collect();
    let value = serde_json::json!({
        "schema_version": 1,
        "action": "sync.clone",
        "remote": receipt.remote,
        "kind": receipt.kind,
        "repo": receipt.repo,
        "path": receipt.path,
        "fetched_count": receipt.fetched_count,
        "remote_ref_count": receipt.remote_ref_count,
        "installed_ref_count": receipt.installed_ref_count,
        "skipped_refs": skipped_refs,
        "filter": fetch_filter_json(receipt.filter),
        "head": receipt.head,
        "checkout": {
            "target": receipt.checkout_target,
            "updated": receipt.worktree_updated,
            "skipped": receipt.checkout_skipped,
        },
        "remote_config": {
            "name": receipt.remote_name,
            "path": receipt.remote_config_path,
        },
    });
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn print_pull_json(receipt: PullJson<'_>) -> anyhow::Result<()> {
    let value = serde_json::json!({
        "schema_version": 1,
        "action": "sync.pull",
        "remote": receipt.remote,
        "ref_name": receipt.ref_name,
        "force": receipt.force,
        "remote_ref_found": receipt.remote_ref_found,
        "remote_revision": receipt.remote_id.map(|id| id.to_string()),
        "fetched_count": receipt.fetched_count,
        "target_available": receipt.target_available,
        "filter": fetch_filter_json(receipt.filter),
        "ref_update": {
            "skipped": receipt.ref_update_skipped,
            "old": receipt.ref_update_old.map(|id| id.to_string()),
            "new": receipt.remote_id.map(|id| id.to_string()),
            "success": receipt.ref_update_success,
            "message": receipt.ref_update_message,
        },
        "worktree": {
            "updated": receipt.worktree_updated,
        },
    });
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn fetch_filter_json(filter: &FetchFilterArgs) -> serde_json::Value {
    serde_json::json!({
        "active": build_fetch_filter(filter).is_some(),
        "dimensions": fetch_filter_dimensions(filter),
        "intent_ids": &filter.intent_ids,
        "path_prefixes": &filter.path_prefixes,
        "codec_ids": &filter.codec_ids,
        "time_start_ms": filter.time_start_ms,
        "time_end_ms": filter.time_end_ms,
        "capsule_visibility": &filter.capsule_visibility,
        "max_depth": filter.max_depth,
        "max_bytes": filter.max_bytes,
        "byte_budget": filter.max_bytes,
    })
}

fn fetch_filter_dimensions(filter: &FetchFilterArgs) -> Vec<&'static str> {
    let mut dimensions = Vec::new();
    if !filter.intent_ids.is_empty() {
        dimensions.push("intent");
    }
    if !filter.path_prefixes.is_empty() {
        dimensions.push("path");
    }
    if filter.time_start_ms.is_some() || filter.time_end_ms.is_some() {
        dimensions.push("time");
    }
    if !filter.codec_ids.is_empty() {
        dimensions.push("codec");
    }
    if filter.capsule_visibility.is_some() {
        dimensions.push("visibility");
    }
    if filter.max_depth.is_some() {
        dimensions.push("depth");
    }
    if filter.max_bytes.is_some() {
        dimensions.push("byte_budget");
    }
    dimensions
}

fn print_push_json(receipt: PushJson<'_>) -> anyhow::Result<()> {
    let ref_update_kind = if receipt.remote_old.is_some() {
        "update"
    } else {
        "create"
    };
    let value = serde_json::json!({
        "schema_version": 1,
        "action": "sync.push",
        "dry_run": receipt.dry_run,
        "remote": receipt.remote,
        "ref_name": receipt.ref_name,
        "force": receipt.force,
        "local_revision": receipt.local_id.to_string(),
        "object_count": receipt.object_count,
        "upload": {
            "skipped": receipt.dry_run,
            "object_count": receipt.object_count,
            "message": receipt.upload_message,
        },
        "ref_update": {
            "skipped": receipt.dry_run,
            "kind": ref_update_kind,
            "old": receipt.remote_old.map(|id| id.to_string()),
            "new": receipt.local_id.to_string(),
            "success": receipt.ref_update_success,
            "message": receipt.ref_update_message,
        },
        "policies": receipt.policy_results.iter().map(push_policy_json).collect::<Vec<_>>(),
        "remote_protocol": {
            "version": SYNC_PROTOCOL_VERSION,
            "capabilities": receipt.remote_capabilities,
        },
    });
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn build_ref_update_context(
    policy_results: &[PushPolicyResult],
    hello: &HelloResponse,
) -> RefUpdateContext {
    let mut requested_capabilities = server_capabilities();
    if !requested_capabilities
        .iter()
        .any(|capability| capability == CAP_POLICY_AWARE_PUSH)
    {
        requested_capabilities.push(CAP_POLICY_AWARE_PUSH.to_string());
    }

    RefUpdateContext {
        policies: policy_results
            .iter()
            .map(|result| RefUpdatePolicyCheck {
                id: result.id.clone(),
                ref_name: result.ref_name.clone(),
                object: result.object.clone(),
                allowed: result.allowed,
                reason: result.reason.clone(),
            })
            .collect(),
        requested_capabilities,
        negotiated_capabilities: hello.capabilities.clone(),
    }
}

fn print_push_policy_denial_json(
    dry_run: bool,
    remote: &str,
    ref_name: &str,
    force: bool,
    local_id: ObjectId,
    policy_results: &[PushPolicyResult],
) -> anyhow::Result<()> {
    let value = serde_json::json!({
        "schema_version": 1,
        "action": "sync.push",
        "dry_run": dry_run,
        "remote": remote,
        "ref_name": ref_name,
        "force": force,
        "local_revision": local_id.to_string(),
        "policy_allowed": false,
        "policies": policy_results.iter().map(push_policy_json).collect::<Vec<_>>(),
        "upload": {
            "skipped": true,
            "object_count": 0,
            "message": "policy denied before upload",
        },
        "ref_update": {
            "skipped": true,
            "kind": "policy-denied",
            "old": null,
            "new": local_id.to_string(),
            "success": false,
            "message": "policy denied before ref update",
        },
    });
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn push_policy_json(result: &PushPolicyResult) -> serde_json::Value {
    serde_json::json!({
        "id": result.id,
        "ref": result.ref_name,
        "object": result.object,
        "allowed": result.allowed,
        "reason": result.reason,
    })
}

fn print_policy_gate_human(results: &[PushPolicyResult]) {
    for result in results {
        if result.allowed {
            println!("  Policy {}: allowed", result.id);
        } else {
            println!(
                "  Policy {}: denied ({})",
                result.id,
                result.reason.as_deref().unwrap_or("unknown reason")
            );
        }
    }
}

fn evaluate_push_policies(
    store: &ClawStore,
    ref_name: &str,
    revision_id: ObjectId,
    policies: &[String],
) -> anyhow::Result<Vec<PushPolicyResult>> {
    if policies.is_empty() {
        return Ok(vec![]);
    }

    let Object::Revision(revision) = store.load_object(&revision_id)? else {
        anyhow::bail!("ref {ref_name} does not point to a revision");
    };
    let (_capsule_id, capsule) = load_default_capsule(store, &revision_id, &revision)?;
    let touched_paths = revision
        .patches
        .iter()
        .filter_map(|patch_id| match store.load_object(patch_id) {
            Ok(Object::Patch(patch)) => Some(patch.target_path),
            _ => None,
        })
        .collect::<Vec<_>>();
    let context = PolicyContext {
        revision_id: Some(revision_id),
        signer_agent_ids: vec![capsule.public_fields.agent_id.clone()],
        signer_key_ids: capsule
            .signatures
            .iter()
            .map(|signature| signature.signer_id.clone())
            .collect(),
        touched_paths,
        trust_score: derive_capsule_trust_score(&capsule),
        now_ms: Some(current_time_ms()),
    };

    policies
        .iter()
        .map(|id| {
            let (policy_ref, policy_object, policy) = load_policy(store, id)?;
            let evaluation = evaluate_policy(&policy, &revision, &capsule, &context);
            Ok(PushPolicyResult {
                id: policy.policy_id,
                ref_name: policy_ref,
                object: policy_object.to_string(),
                allowed: evaluation.is_ok(),
                reason: evaluation.err().map(|err| err.to_string()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{
        build_fetch_filter, build_grpc_tls_config, check_remote_compatibility, fetch_filter_json,
        resolve_command, resolve_token_profiles, SyncArgs, SyncCommand, CLI_VERSION,
    };
    use claw_sync::proto::sync::HelloResponse;
    use claw_sync::protocol::CAP_PROTOCOL_V1;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: SyncArgs,
    }

    #[test]
    fn parse_compat_remote_form() {
        let cli = TestCli::parse_from(["claw", "origin"]);

        match resolve_command(cli.args) {
            SyncCommand::Pull {
                remote,
                ref_name,
                force,
                ..
            } => {
                assert_eq!(remote, "origin");
                assert_eq!(ref_name, "heads/main");
                assert!(!force);
            }
            _ => panic!("expected pull command"),
        }
    }

    #[test]
    fn parse_pull_subcommand_form() {
        let cli = TestCli::parse_from(["claw", "pull", "--remote", "upstream"]);

        match resolve_command(cli.args) {
            SyncCommand::Pull {
                remote,
                ref_name,
                force,
                ..
            } => {
                assert_eq!(remote, "upstream");
                assert_eq!(ref_name, "heads/main");
                assert!(!force);
            }
            _ => panic!("expected pull command"),
        }
    }

    #[test]
    fn parse_push_dry_run() {
        let cli = TestCli::parse_from(["claw", "push", "--remote", "origin", "--dry-run"]);

        match resolve_command(cli.args) {
            SyncCommand::Push {
                remote,
                ref_name,
                json,
                force,
                dry_run,
                policies,
            } => {
                assert_eq!(remote, "origin");
                assert_eq!(ref_name, "heads/main");
                assert!(!json);
                assert!(!force);
                assert!(dry_run);
                assert!(policies.is_empty());
            }
            _ => panic!("expected push command"),
        }
    }

    #[test]
    fn parse_push_json() {
        let cli = TestCli::parse_from([
            "claw", "push", "--json", "--remote", "origin", "--policy", "release",
        ]);

        match resolve_command(cli.args) {
            SyncCommand::Push { json, policies, .. } => {
                assert!(json);
                assert_eq!(policies, vec!["release"]);
            }
            _ => panic!("expected push command"),
        }
    }

    #[test]
    fn parse_pull_partial_clone_filter() {
        let cli = TestCli::parse_from([
            "claw",
            "pull",
            "--intent",
            "01H00000000000000000000000",
            "--path-prefix",
            "src/",
            "--codec",
            "text/line",
            "--visibility",
            "public",
            "--depth",
            "3",
            "--byte-budget",
            "4096",
            "--time-start-ms",
            "1000",
            "--time-end-ms",
            "2000",
        ]);

        match resolve_command(cli.args) {
            SyncCommand::Pull { filter, .. } => {
                let proto = build_fetch_filter(&filter).expect("filter should be present");
                assert_eq!(proto.intent_ids, vec!["01H00000000000000000000000"]);
                assert_eq!(proto.path_prefixes, vec!["src/"]);
                assert_eq!(proto.codec_ids, vec!["text/line"]);
                assert_eq!(proto.capsule_visibility, "public");
                assert_eq!(proto.max_depth, 3);
                assert_eq!(proto.max_bytes, 4096);
                assert_eq!(proto.time_range_start, 1000);
                assert_eq!(proto.time_range_end, 2000);
                let filter_json = fetch_filter_json(&filter);
                assert_eq!(
                    filter_json["dimensions"],
                    serde_json::json!([
                        "intent",
                        "path",
                        "time",
                        "codec",
                        "visibility",
                        "depth",
                        "byte_budget"
                    ])
                );
                assert_eq!(filter_json["byte_budget"], 4096);
            }
            _ => panic!("expected pull command"),
        }
    }

    #[test]
    fn parse_clone_partial_clone_filter() {
        let cli = TestCli::parse_from([
            "claw",
            "clone",
            "http://127.0.0.1:50051",
            "./partial",
            "--intent",
            "01H00000000000000000000000",
            "--path-prefix",
            "src/",
            "--codec",
            "json/tree",
            "--visibility",
            "private",
            "--depth",
            "2",
            "--bytes",
            "8192",
            "--time-start-ms",
            "3000",
            "--time-end-ms",
            "4000",
            "--json",
        ]);

        match resolve_command(cli.args) {
            SyncCommand::Clone {
                filter, path, json, ..
            } => {
                assert_eq!(path, "./partial");
                assert!(json);
                let proto = build_fetch_filter(&filter).expect("filter should be present");
                assert_eq!(proto.intent_ids, vec!["01H00000000000000000000000"]);
                assert_eq!(proto.path_prefixes, vec!["src/"]);
                assert_eq!(proto.codec_ids, vec!["json/tree"]);
                assert_eq!(proto.capsule_visibility, "private");
                assert_eq!(proto.max_depth, 2);
                assert_eq!(proto.max_bytes, 8192);
                assert_eq!(proto.time_range_start, 3000);
                assert_eq!(proto.time_range_end, 4000);
            }
            _ => panic!("expected clone command"),
        }
    }

    #[test]
    fn parse_sync_without_remote_defaults_to_origin() {
        let cli = TestCli::parse_from(["claw"]);

        match resolve_command(cli.args) {
            SyncCommand::Pull {
                remote,
                ref_name,
                force,
                ..
            } => {
                assert_eq!(remote, "origin");
                assert_eq!(ref_name, "heads/main");
                assert!(!force);
            }
            _ => panic!("expected pull command"),
        }
    }

    #[test]
    fn compat_check_accepts_matching_version() {
        let hello = HelloResponse {
            server_version: CLI_VERSION.to_string(),
            capabilities: vec![CAP_PROTOCOL_V1.to_string(), "partial-clone".to_string()],
        };

        check_remote_compatibility("origin", &hello).expect("expected compatible versions");
    }

    #[test]
    fn compat_check_accepts_n_minus_one_minor_version() {
        let mut parts = CLI_VERSION.split('.');
        let major: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let minor: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let n_minus_one = minor.saturating_sub(1);
        let hello = HelloResponse {
            server_version: format!("{major}.{n_minus_one}.99"),
            capabilities: vec![CAP_PROTOCOL_V1.to_string(), "partial-clone".to_string()],
        };

        check_remote_compatibility("origin", &hello)
            .expect("N/N-1 compatibility should be accepted");
    }

    #[test]
    fn compat_check_rejects_unsupported_version_gap() {
        let hello = HelloResponse {
            server_version: "9.9.9".to_string(),
            capabilities: vec![CAP_PROTOCOL_V1.to_string(), "partial-clone".to_string()],
        };

        let err = check_remote_compatibility("origin", &hello).expect_err("expected mismatch");
        let message = err.to_string();
        assert!(message.contains("compatibility check failed"));
        assert!(message.contains("incompatible"));
        assert!(message.contains("N/N-1"));
        assert!(message.contains("--no-compat-check"));
    }

    #[test]
    fn compat_check_rejects_missing_protocol_marker() {
        let hello = HelloResponse {
            server_version: CLI_VERSION.to_string(),
            capabilities: vec!["partial-clone".to_string()],
        };

        let err =
            check_remote_compatibility("origin", &hello).expect_err("expected protocol mismatch");
        assert!(err.to_string().contains("protocol negotiation failed"));
    }

    #[test]
    fn parse_sync_mtls_flags() {
        let cli = TestCli::parse_from([
            "claw",
            "--tls-ca-cert",
            "ca.pem",
            "--tls-domain",
            "claw.example",
            "--client-cert",
            "client.pem",
            "--client-key",
            "client-key.pem",
            "push",
        ]);

        assert_eq!(
            cli.args.tls_ca_cert.as_deref(),
            Some(std::path::Path::new("ca.pem"))
        );
        assert_eq!(cli.args.tls_domain.as_deref(), Some("claw.example"));
        assert_eq!(
            cli.args.client_cert.as_deref(),
            Some(std::path::Path::new("client.pem"))
        );
        assert_eq!(
            cli.args.client_key.as_deref(),
            Some(std::path::Path::new("client-key.pem"))
        );
    }

    #[test]
    fn mtls_config_requires_cert_and_key_pair() {
        let cli = TestCli::parse_from(["claw", "--client-cert", "client.pem", "pull"]);

        let err = build_grpc_tls_config(&cli.args).expect_err("missing key should fail");
        assert!(err.to_string().contains("--client-cert and --client-key"));
    }

    #[test]
    fn resolve_token_profiles_prefers_explicit_profile() {
        let profiles = resolve_token_profiles(Some("team-ci"), "prod", "default");

        assert_eq!(profiles, vec!["team-ci"]);
    }

    #[test]
    fn resolve_token_profiles_uses_runtime_then_repo_default_when_omitted() {
        let profiles = resolve_token_profiles(None, "prod", "default");

        assert_eq!(profiles, vec!["prod", "default"]);
    }

    #[test]
    fn resolve_token_profiles_deduplicates_runtime_and_repo_default() {
        let profiles = resolve_token_profiles(None, "default", "default");

        assert_eq!(profiles, vec!["default"]);
    }
}
