use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use claw_store::ClawStore;

use crate::auth_store;
use crate::commands::RuntimeOptions;
use crate::config::{self, ClawConfigV1};

const ADMIN_JSON_SCHEMA_VERSION: u8 = 1;

#[derive(Args)]
pub struct AdminArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: AdminCommand,
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Validate host and repository production preconditions
    Preflight,
    /// Manage backups
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    /// Plan/apply config migrations
    Migrate {
        #[command(subcommand)]
        command: MigrateCommand,
    },
    /// Plan/execute rollback from backups
    Rollback {
        #[command(subcommand)]
        command: RollbackCommand,
    },
    /// Produce a diagnostic support bundle
    SupportBundle {
        /// Output path for support bundle JSON
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum BackupCommand {
    /// Create a repository metadata backup
    Create,
    /// Verify backup integrity
    Verify {
        /// Backup id to verify (defaults to latest)
        #[arg(long)]
        backup_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum MigrateCommand {
    /// Show migration plan and config diff preview
    Plan,
    /// Apply config migration (or preview with --dry-run)
    Apply {
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum RollbackCommand {
    /// Show rollback plan for a backup
    Plan {
        /// Backup id to rollback to (defaults to latest)
        #[arg(long)]
        backup_id: Option<String>,
    },
    /// Execute rollback from backup snapshot
    Execute {
        /// Backup id to rollback to (defaults to latest)
        #[arg(long)]
        backup_id: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
struct BackupManifest {
    backup_id: String,
    created_at_ms: u64,
    files: Vec<BackupEntry>,
}

#[derive(Serialize, serde::Deserialize)]
struct BackupEntry {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct BackupVerification {
    backup_id: String,
    manifest_path: String,
    snapshot_path: String,
    file_count: usize,
    total_bytes: u64,
    verified: bool,
}

#[derive(Serialize)]
struct LedgerEntry {
    timestamp_ms: u64,
    action: String,
    status: String,
    details: serde_json::Value,
}

#[derive(Serialize)]
struct SupportBundle {
    schema_version: u8,
    action: &'static str,
    request_id: String,
    created_at_ms: u64,
    repo_root: String,
    config: serde_json::Value,
    redactions: Vec<String>,
    head: Option<String>,
    refs_count: usize,
    latest_backup_id: Option<String>,
}

pub fn run(args: AdminArgs, runtime: &RuntimeOptions) -> anyhow::Result<()> {
    let json = args.json;
    match args.command {
        AdminCommand::Preflight => run_preflight(runtime, json),
        AdminCommand::Backup { command } => match command {
            BackupCommand::Create => run_backup_create(json),
            BackupCommand::Verify { backup_id } => run_backup_verify(backup_id, json),
        },
        AdminCommand::Migrate { command } => match command {
            MigrateCommand::Plan => run_migrate_plan(json),
            MigrateCommand::Apply { dry_run } => run_migrate_apply(dry_run, json),
        },
        AdminCommand::Rollback { command } => match command {
            RollbackCommand::Plan { backup_id } => run_rollback_plan(backup_id, json),
            RollbackCommand::Execute { backup_id } => run_rollback_execute(backup_id, json),
        },
        AdminCommand::SupportBundle { out } => run_support_bundle(out, json),
    }
}

fn run_preflight(_runtime: &RuntimeOptions, json: bool) -> anyhow::Result<()> {
    const DISK_FREE_WARN_KIB: u64 = 1_048_576; // 1 GiB
    const DISK_FREE_FAIL_KIB: u64 = 262_144; // 256 MiB
    const NOFILE_WARN_SOFT_LIMIT: u64 = 8_192;
    const NOFILE_FAIL_SOFT_LIMIT: u64 = 1_024;

    let root = config::find_repo_root()?;
    let claw_dir = root.join(".claw");
    let cfg = config::load_or_default_config(&root)?;

    let mut checks = Vec::new();

    if !claw_dir.is_dir() {
        checks.push(CheckResult::fail(
            "metadata directory",
            format!(
                "missing repository metadata directory: {}",
                claw_dir.display()
            ),
            "create the .claw directory or re-initialize the repository metadata",
        ));
    } else {
        checks.push(CheckResult::pass(
            "metadata directory",
            format!("found {}", claw_dir.display()),
        ));
    }

    if claw_dir.is_dir() {
        match fsync_probe(&claw_dir) {
            Ok(()) => checks.push(CheckResult::pass(
                "metadata fsync",
                "probe file write+sync succeeded".to_string(),
            )),
            Err(err) => checks.push(CheckResult::fail(
                "metadata fsync",
                format!("probe file write+sync failed: {err}"),
                "verify disk health and mount options, then ensure the process can write and fsync inside .claw",
            )),
        }

        match disk_free_kib(&claw_dir) {
            Ok(free_kib) => {
                let disk_status =
                    evaluate_threshold(free_kib, DISK_FREE_WARN_KIB, DISK_FREE_FAIL_KIB);
                let free_mib = free_kib / 1024;
                match disk_status {
                    ThresholdStatus::Pass => checks.push(CheckResult::pass(
                        "metadata disk space",
                        format!("{free_mib} MiB available on .claw filesystem"),
                    )),
                    ThresholdStatus::Warn => checks.push(CheckResult::warn(
                        "metadata disk space",
                        format!(
                            "{free_mib} MiB available (< {} MiB warning threshold)",
                            DISK_FREE_WARN_KIB / 1024
                        ),
                        "free up space or move repository metadata to a filesystem with more capacity",
                    )),
                    ThresholdStatus::Fail => checks.push(CheckResult::fail(
                        "metadata disk space",
                        format!(
                            "{free_mib} MiB available (< {} MiB fail threshold)",
                            DISK_FREE_FAIL_KIB / 1024
                        ),
                        "free disk space immediately before running production workloads",
                    )),
                }
            }
            Err(err) => checks.push(CheckResult::warn(
                "metadata disk space",
                format!("unable to measure free space: {err}"),
                "run `df -h .claw` manually and ensure available space exceeds deployment needs",
            )),
        }

        match nofile_soft_limit() {
            Ok(limit) => {
                if limit == u64::MAX {
                    checks.push(CheckResult::pass(
                        "open file descriptor limit (nofile)",
                        "soft limit is unlimited".to_string(),
                    ));
                } else {
                    match evaluate_threshold(limit, NOFILE_WARN_SOFT_LIMIT, NOFILE_FAIL_SOFT_LIMIT) {
                        ThresholdStatus::Pass => checks.push(CheckResult::pass(
                            "open file descriptor limit (nofile)",
                            format!("soft limit is {limit}"),
                        )),
                        ThresholdStatus::Warn => checks.push(CheckResult::warn(
                            "open file descriptor limit (nofile)",
                            format!(
                                "soft limit is {limit} (< {NOFILE_WARN_SOFT_LIMIT} warning threshold)"
                            ),
                            "raise the soft limit (for example `ulimit -n 8192`) before high-concurrency runs",
                        )),
                        ThresholdStatus::Fail => checks.push(CheckResult::fail(
                            "open file descriptor limit (nofile)",
                            format!(
                                "soft limit is {limit} (< {NOFILE_FAIL_SOFT_LIMIT} fail threshold)"
                            ),
                            "increase file descriptor limits in shell/system configuration and re-run preflight",
                        )),
                    }
                }
            }
            Err(err) => checks.push(CheckResult::warn(
                "open file descriptor limit (nofile)",
                format!("unable to read soft limit: {err}"),
                "check `ulimit -n` manually and ensure it is sized for production concurrency",
            )),
        }
    }

    let selected_profile = config::default_profile(&cfg).to_string();
    if !cfg.auth.require_auth_for_daemon {
        checks.push(CheckResult::pass(
            "daemon auth token",
            "not required by current configuration".to_string(),
        ));
    } else {
        match auth_store::try_resolve_access_token(Some(&selected_profile)) {
            Ok(Some(_)) => checks.push(CheckResult::pass(
                "daemon auth token",
                format!("token present for profile '{selected_profile}'"),
            )),
            Ok(None) => checks.push(CheckResult::warn(
                "daemon auth token",
                format!(
                    "no auth token found for profile '{selected_profile}' (required for production daemon auth)"
                ),
                "run `claw auth login --profile <profile>` before starting the production daemon",
            )),
            Err(err) => checks.push(CheckResult::fail(
                "daemon auth token",
                format!("unable to read auth profile '{selected_profile}': {err}"),
                "fix ~/.claw/auth.toml or recreate the profile with `claw auth token set --stdin`",
            )),
        }
    }

    if cfg.tls.require_for_non_localhost {
        match (&cfg.tls.cert_path, &cfg.tls.key_path) {
            (Some(_), None) | (None, Some(_)) => checks.push(CheckResult::fail(
                "tls configuration",
                "tls.cert_path and tls.key_path must both be set when TLS is configured"
                    .to_string(),
                "set both TLS paths in config or disable tls.require_for_non_localhost for local-only usage",
            )),
            _ => checks.push(CheckResult::pass(
                "tls configuration",
                "TLS settings are consistent".to_string(),
            )),
        }
    } else {
        checks.push(CheckResult::pass(
            "tls configuration",
            "TLS is not required for non-localhost by current configuration".to_string(),
        ));
    }

    let has_failures = checks.iter().any(|check| check.status == CheckStatus::Fail);

    if json {
        let pass_count = checks
            .iter()
            .filter(|check| check.status == CheckStatus::Pass)
            .count();
        let warn_count = checks
            .iter()
            .filter(|check| check.status == CheckStatus::Warn)
            .count();
        let fail_count = checks
            .iter()
            .filter(|check| check.status == CheckStatus::Fail)
            .count();
        print_json(serde_json::json!({
            "schema_version": ADMIN_JSON_SCHEMA_VERSION,
            "action": "preflight",
            "ok": !has_failures,
            "repo_root": root.display().to_string(),
            "config_path": config::config_file_path(&root).display().to_string(),
            "summary": {
                "pass": pass_count,
                "warn": warn_count,
                "fail": fail_count,
            },
            "checks": checks.iter().map(CheckResult::to_json).collect::<Vec<_>>(),
        }))?;
    } else {
        if has_failures {
            println!("Preflight: FAIL");
        } else {
            println!("Preflight: PASS");
        }

        for check in &checks {
            println!("  {}", check.render());
        }

        println!("  repository: {}", root.display());
        println!("  config: {}", config::config_file_path(&root).display());
    }

    if has_failures {
        anyhow::bail!("preflight failed");
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThresholdStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug)]
struct CheckResult {
    status: CheckStatus,
    name: &'static str,
    detail: String,
    next_step: Option<&'static str>,
}

impl CheckResult {
    fn pass(name: &'static str, detail: String) -> Self {
        Self {
            status: CheckStatus::Pass,
            name,
            detail,
            next_step: None,
        }
    }

    fn warn(name: &'static str, detail: String, next_step: &'static str) -> Self {
        Self {
            status: CheckStatus::Warn,
            name,
            detail,
            next_step: Some(next_step),
        }
    }

    fn fail(name: &'static str, detail: String, next_step: &'static str) -> Self {
        Self {
            status: CheckStatus::Fail,
            name,
            detail,
            next_step: Some(next_step),
        }
    }

    fn render(&self) -> String {
        let status_label = match self.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        match self.next_step {
            Some(step) => format!(
                "[{status_label}] {}: {} | next: {step}",
                self.name, self.detail
            ),
            None => format!("[{status_label}] {}: {}", self.name, self.detail),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "status": self.status.as_str(),
            "detail": self.detail,
            "next_step": self.next_step,
        })
    }
}

fn evaluate_threshold(value: u64, warn_threshold: u64, fail_threshold: u64) -> ThresholdStatus {
    if value < fail_threshold {
        ThresholdStatus::Fail
    } else if value < warn_threshold {
        ThresholdStatus::Warn
    } else {
        ThresholdStatus::Pass
    }
}

fn fsync_probe(claw_dir: &Path) -> anyhow::Result<()> {
    let path = claw_dir.join("preflight-fsync-probe.tmp");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)?;
    file.write_all(b"probe")?;
    file.sync_all()?;
    std::fs::remove_file(path)?;
    Ok(())
}

fn disk_free_kib(path: &Path) -> anyhow::Result<u64> {
    let output = Command::new("df").arg("-Pk").arg(path).output()?;
    if !output.status.success() {
        anyhow::bail!("df exited with status {}", output.status);
    }

    let stdout = String::from_utf8(output.stdout)?;
    let data_line = stdout
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("df output missing filesystem data row"))?;
    let columns: Vec<&str> = data_line.split_whitespace().collect();
    if columns.len() < 4 {
        anyhow::bail!("unexpected df output row: {data_line}");
    }

    columns[3]
        .parse::<u64>()
        .map_err(|err| anyhow::anyhow!("invalid df available column '{}': {err}", columns[3]))
}

fn nofile_soft_limit() -> anyhow::Result<u64> {
    let output = Command::new("sh").arg("-c").arg("ulimit -Sn").output()?;
    if !output.status.success() {
        anyhow::bail!("ulimit exited with status {}", output.status);
    }

    let value = String::from_utf8(output.stdout)?.trim().to_string();
    if value.eq_ignore_ascii_case("unlimited") {
        return Ok(u64::MAX);
    }

    value
        .parse::<u64>()
        .map_err(|err| anyhow::anyhow!("invalid nofile soft limit '{value}': {err}"))
}

fn run_backup_create(json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let backup = create_backup(&root)?;
    let total_bytes = backup.files.iter().map(|entry| entry.bytes).sum::<u64>();
    append_ledger(
        &root,
        LedgerEntry {
            timestamp_ms: now_ms(),
            action: "backup.create".to_string(),
            status: "ok".to_string(),
            details: serde_json::json!({
                "backup_id": backup.backup_id,
                "file_count": backup.files.len(),
                "total_bytes": total_bytes,
            }),
        },
    )?;
    if json {
        print_json(serde_json::json!({
            "schema_version": ADMIN_JSON_SCHEMA_VERSION,
            "action": "backup.create",
            "backup_id": backup.backup_id,
            "created_at_ms": backup.created_at_ms,
            "file_count": backup.files.len(),
            "total_bytes": total_bytes,
        }))?;
    } else {
        println!("Created backup: {}", backup.backup_id);
        println!("  files: {}", backup.files.len());
    }
    Ok(())
}

fn run_backup_verify(backup_id: Option<String>, json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let backup_id = resolve_backup_id(&root, backup_id.as_deref())?;
    let verification = verify_backup(&root, &backup_id)?;
    if json {
        print_json(serde_json::json!({
            "schema_version": ADMIN_JSON_SCHEMA_VERSION,
            "action": "backup.verify",
            "backup_id": backup_id,
            "backup": verification,
        }))?;
    } else {
        println!("Backup verified: {backup_id}");
    }
    Ok(())
}

fn run_migrate_plan(json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let plan = config::plan_config_migration(&root)?;
    if json {
        print_json(migration_plan_json(
            "migrate.plan",
            false,
            false,
            None,
            &plan,
        ))?;
    } else {
        println!("Migration plan -> {}", plan.target.display());
        if let Some(source) = &plan.source {
            println!("  source: {}", source.display());
        } else {
            println!("  source: defaults");
        }
        println!("{}", plan.diff);
    }
    Ok(())
}

fn run_migrate_apply(dry_run: bool, json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let plan = config::plan_config_migration(&root)?;
    if dry_run {
        if json {
            print_json(migration_plan_json(
                "migrate.apply",
                true,
                false,
                None,
                &plan,
            ))?;
        } else {
            println!("Migration target: {}", plan.target.display());
            println!("{}", plan.diff);
            println!("Dry run complete. No files changed.");
        }
        return Ok(());
    }

    let backup = create_backup(&root)?;
    let backup_id = backup.backup_id.clone();
    let applied = config::apply_config_migration(&root)?;
    append_ledger(
        &root,
        LedgerEntry {
            timestamp_ms: now_ms(),
            action: "migrate.apply".to_string(),
            status: "ok".to_string(),
            details: serde_json::json!({
                "backup_id": backup_id.clone(),
                "target": applied.target.display().to_string(),
            }),
        },
    )?;
    if json {
        print_json(migration_plan_json(
            "migrate.apply",
            false,
            true,
            Some(backup_id.as_str()),
            &applied,
        ))?;
    } else {
        println!("Migration target: {}", plan.target.display());
        println!("{}", plan.diff);
        println!("Migration applied.");
    }
    Ok(())
}

fn migration_plan_json(
    action: &'static str,
    dry_run: bool,
    applied: bool,
    backup_id: Option<&str>,
    plan: &config::ConfigMigrationPlan,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": ADMIN_JSON_SCHEMA_VERSION,
        "action": action,
        "dry_run": dry_run,
        "applied": applied,
        "backup_id": backup_id,
        "target": plan.target.display().to_string(),
        "source": plan.source.as_ref().map(|source| source.display().to_string()),
        "source_kind": if plan.source.is_some() { "file" } else { "defaults" },
        "diff": plan.diff.as_str(),
    })
}

fn run_rollback_plan(backup_id: Option<String>, json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let backup_id = resolve_backup_id(&root, backup_id.as_deref())?;
    let verification = verify_backup(&root, &backup_id)?;
    let snapshot = snapshot_dir(&root, &backup_id);
    let files = collect_relative_files(&snapshot)?;
    if json {
        print_json(serde_json::json!({
            "schema_version": ADMIN_JSON_SCHEMA_VERSION,
            "action": "rollback.plan",
            "backup_id": backup_id,
            "verified": verification.verified,
            "restore_file_count": files.len(),
        }))?;
    } else {
        println!("Rollback plan from backup: {backup_id}");
        println!("  restore files: {}", files.len());
    }
    Ok(())
}

fn run_rollback_execute(backup_id: Option<String>, json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let backup_id = resolve_backup_id(&root, backup_id.as_deref())?;
    let verification = verify_backup(&root, &backup_id)?;
    restore_backup(&root, &backup_id)?;
    append_ledger(
        &root,
        LedgerEntry {
            timestamp_ms: now_ms(),
            action: "rollback.execute".to_string(),
            status: "ok".to_string(),
            details: serde_json::json!({"backup_id": backup_id}),
        },
    )?;
    if json {
        print_json(serde_json::json!({
            "schema_version": ADMIN_JSON_SCHEMA_VERSION,
            "action": "rollback.execute",
            "backup_id": backup_id,
            "verified": verification.verified,
            "restored": true,
        }))?;
    } else {
        println!("Rollback executed: {backup_id}");
    }
    Ok(())
}

fn run_support_bundle(out: Option<PathBuf>, json: bool) -> anyhow::Result<()> {
    let root = config::find_repo_root()?;
    let cfg = config::load_or_default_config(&root)?;
    let store = ClawStore::open(&root)?;

    let refs_count = store.list_refs("")?.len();
    let head = store.read_head().ok().map(|v| format!("{v:?}"));
    let latest_backup_id = resolve_backup_id(&root, None).ok();
    let request_id = format!("req_{}", now_ms());
    let (config, redactions) = support_bundle_config(&cfg)?;

    let bundle = SupportBundle {
        schema_version: ADMIN_JSON_SCHEMA_VERSION,
        action: "support-bundle",
        request_id: request_id.clone(),
        created_at_ms: now_ms(),
        repo_root: root.display().to_string(),
        config,
        redactions,
        head,
        refs_count,
        latest_backup_id,
    };

    let output_path = out.unwrap_or_else(|| {
        root.join(".claw")
            .join("support")
            .join(format!("support-bundle-{request_id}.json"))
    });
    let content = serde_json::to_string_pretty(&bundle)?;
    write_bytes_atomic(&output_path, content.as_bytes())?;

    append_ledger(
        &root,
        LedgerEntry {
            timestamp_ms: now_ms(),
            action: "support-bundle".to_string(),
            status: "ok".to_string(),
            details: serde_json::json!({"path": output_path.display().to_string()}),
        },
    )?;

    if json {
        print_json(serde_json::json!({
            "schema_version": ADMIN_JSON_SCHEMA_VERSION,
            "action": "support-bundle",
            "written": true,
            "path": output_path.display().to_string(),
            "request_id": bundle.request_id,
            "created_at_ms": bundle.created_at_ms,
            "refs_count": bundle.refs_count,
            "latest_backup_id": bundle.latest_backup_id,
            "redaction_count": bundle.redactions.len(),
        }))?;
    } else {
        println!("Support bundle written: {}", output_path.display());
    }
    Ok(())
}

fn support_bundle_config(cfg: &ClawConfigV1) -> anyhow::Result<(serde_json::Value, Vec<String>)> {
    let mut value = serde_json::to_value(cfg)?;
    let mut redactions = Vec::new();
    if let Some(tls) = value.get_mut("tls").and_then(|tls| tls.as_object_mut()) {
        for field in ["cert_path", "key_path"] {
            if tls.get(field).is_some_and(|path| !path.is_null()) {
                tls.insert(
                    field.to_string(),
                    serde_json::Value::String("<redacted:support-bundle>".to_string()),
                );
                redactions.push(format!("config.tls.{field}"));
            }
        }
    }
    Ok((value, redactions))
}

fn create_backup(root: &Path) -> anyhow::Result<BackupManifest> {
    let backup_id = now_ms().to_string();
    let backup_root = backups_dir(root).join(&backup_id);
    let snapshot_root = backup_root.join("snapshot");
    std::fs::create_dir_all(&snapshot_root)?;

    let source = root.join(".claw");
    let mut entries = Vec::new();
    copy_tree_with_hashes(&source, &source, &snapshot_root, &mut entries)?;

    let manifest = BackupManifest {
        backup_id: backup_id.clone(),
        created_at_ms: now_ms(),
        files: entries,
    };
    let manifest_path = backup_root.join("manifest.json");
    write_bytes_atomic(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
    Ok(manifest)
}

fn verify_backup(root: &Path, backup_id: &str) -> anyhow::Result<BackupVerification> {
    let manifest_path = backups_dir(root).join(backup_id).join("manifest.json");
    let manifest: BackupManifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)
        .map_err(|err| {
            anyhow::anyhow!("invalid backup manifest {}: {err}", manifest_path.display())
        })?;

    let snapshot = snapshot_dir(root, backup_id);
    let total_bytes = manifest.files.iter().map(|entry| entry.bytes).sum::<u64>();
    let file_count = manifest.files.len();
    for entry in &manifest.files {
        let path = snapshot.join(&entry.path);
        let bytes = std::fs::read(&path)
            .map_err(|err| anyhow::anyhow!("backup file missing {}: {err}", path.display()))?;
        if bytes.len() as u64 != entry.bytes {
            anyhow::bail!("backup size mismatch for {}", entry.path);
        }
        let hash = sha256_hex(&bytes);
        if hash != entry.sha256 {
            anyhow::bail!("backup checksum mismatch for {}", entry.path);
        }
    }
    Ok(BackupVerification {
        backup_id: backup_id.to_string(),
        manifest_path: manifest_path.display().to_string(),
        snapshot_path: snapshot.display().to_string(),
        file_count,
        total_bytes,
        verified: true,
    })
}

fn restore_backup(root: &Path, backup_id: &str) -> anyhow::Result<()> {
    let snapshot = snapshot_dir(root, backup_id);
    let target = root.join(".claw");

    let snapshot_files = collect_relative_files(&snapshot)?;
    let target_files = collect_relative_files(&target)?;

    for rel in target_files.difference(&snapshot_files) {
        if rel.starts_with("backups/") {
            continue;
        }
        let path = target.join(rel);
        if path.is_file() {
            std::fs::remove_file(path)?;
        }
    }

    for rel in snapshot_files {
        if rel.starts_with("backups/") {
            continue;
        }
        let source = snapshot.join(&rel);
        let dest = target.join(&rel);
        copy_file_durable(&source, &dest)?;
    }

    Ok(())
}

fn copy_tree_with_hashes(
    source_root: &Path,
    current: &Path,
    destination_root: &Path,
    entries: &mut Vec<BackupEntry>,
) -> anyhow::Result<()> {
    let mut dir_entries: Vec<std::fs::DirEntry> =
        std::fs::read_dir(current)?.collect::<Result<_, _>>()?;
    dir_entries.sort_by_key(|entry| entry.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let rel = path
            .strip_prefix(source_root)
            .map_err(|err| anyhow::anyhow!("backup path error: {err}"))?;

        if rel.starts_with("backups") {
            continue;
        }

        let dest = destination_root.join(rel);
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&dest)?;
            copy_tree_with_hashes(source_root, &path, destination_root, entries)?;
            continue;
        }

        copy_file_durable(&path, &dest)?;
        let bytes = std::fs::read(&path)?;
        let rel_path = dest
            .strip_prefix(destination_root)
            .map_err(|err| anyhow::anyhow!("backup relative path error: {err}"))?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(BackupEntry {
            path: rel_path,
            bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }

    Ok(())
}

fn collect_relative_files(root: &Path) -> anyhow::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    if !root.exists() {
        return Ok(out);
    }
    collect_relative_files_inner(root, root, &mut out)?;
    Ok(out)
}

fn collect_relative_files_inner(
    root: &Path,
    current: &Path,
    out: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_relative_files_inner(root, &path, out)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|err| anyhow::anyhow!("relative file path error: {err}"))?
            .to_string_lossy()
            .replace('\\', "/");
        out.insert(rel);
    }
    Ok(())
}

fn append_ledger(root: &Path, entry: LedgerEntry) -> anyhow::Result<()> {
    let path = root.join(".claw").join("migrations").join("ledger.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(&entry)?;
    line.push(b'\n');

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&line)?;
    file.sync_all()?;
    Ok(())
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    temp.persist(path).map_err(|err| err.error)?;
    sync_dir(parent)?;
    Ok(())
}

fn copy_file_durable(source: &Path, dest: &Path) -> anyhow::Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", dest.display()))?;
    std::fs::create_dir_all(parent)?;
    std::fs::copy(source, dest)?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dest)?
        .sync_all()?;
    sync_dir(parent)?;
    Ok(())
}

fn sync_dir(path: &Path) -> anyhow::Result<()> {
    if let Ok(dir) = std::fs::File::open(path) {
        match dir.sync_all() {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn backups_dir(root: &Path) -> PathBuf {
    root.join(".claw").join("backups")
}

fn snapshot_dir(root: &Path, backup_id: &str) -> PathBuf {
    backups_dir(root).join(backup_id).join("snapshot")
}

fn resolve_backup_id(root: &Path, backup_id: Option<&str>) -> anyhow::Result<String> {
    if let Some(id) = backup_id {
        return Ok(id.to_string());
    }

    let mut candidates = Vec::new();
    let dir = backups_dir(root);
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                candidates.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    candidates.sort();
    candidates
        .pop()
        .ok_or_else(|| anyhow::anyhow!("no backups found"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn print_json(value: serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| dur.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{
        create_backup, evaluate_threshold, verify_backup, AdminArgs, AdminCommand, BackupCommand,
        MigrateCommand, RollbackCommand, ThresholdStatus,
    };

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: AdminArgs,
    }

    #[test]
    fn parse_backup_verify_with_backup_id() {
        let cli = TestCli::parse_from(["claw", "backup", "verify", "--backup-id", "1700000000000"]);

        match cli.args.command {
            AdminCommand::Backup { command } => match command {
                BackupCommand::Verify { backup_id } => {
                    assert_eq!(backup_id.as_deref(), Some("1700000000000"));
                }
                _ => panic!("expected backup verify command"),
            },
            _ => panic!("expected backup command"),
        }
    }

    #[test]
    fn parse_migrate_apply_dry_run() {
        let cli = TestCli::parse_from(["claw", "migrate", "apply", "--dry-run"]);

        match cli.args.command {
            AdminCommand::Migrate { command } => match command {
                MigrateCommand::Apply { dry_run } => {
                    assert!(dry_run);
                }
                _ => panic!("expected migrate apply command"),
            },
            _ => panic!("expected migrate command"),
        }
    }

    #[test]
    fn parse_rollback_execute_with_backup_id() {
        let cli = TestCli::parse_from([
            "claw",
            "rollback",
            "execute",
            "--backup-id",
            "1700000000000",
        ]);

        match cli.args.command {
            AdminCommand::Rollback { command } => match command {
                RollbackCommand::Execute { backup_id } => {
                    assert_eq!(backup_id.as_deref(), Some("1700000000000"));
                }
                _ => panic!("expected rollback execute command"),
            },
            _ => panic!("expected rollback command"),
        }
    }

    #[test]
    fn backup_create_and_verify_happy_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let claw_dir = root.join(".claw");

        std::fs::create_dir_all(claw_dir.join("refs")).expect("create refs");
        std::fs::create_dir_all(claw_dir.join("objects")).expect("create objects");
        std::fs::create_dir_all(claw_dir.join("backups").join("legacy").join("snapshot"))
            .expect("create legacy backup tree");

        std::fs::write(claw_dir.join("refs").join("heads_main"), b"main").expect("write ref");
        std::fs::write(claw_dir.join("objects").join("obj1"), b"payload").expect("write object");
        std::fs::write(
            claw_dir
                .join("backups")
                .join("legacy")
                .join("snapshot")
                .join("ignored.txt"),
            b"do-not-include",
        )
        .expect("write ignored backup file");

        let manifest = create_backup(root).expect("create backup");
        assert!(!manifest.files.is_empty());
        assert!(manifest
            .files
            .iter()
            .all(|entry| !entry.path.starts_with("backups/")));

        verify_backup(root, &manifest.backup_id).expect("verify backup");
    }

    #[test]
    fn threshold_evaluation_pass_at_warning_boundary() {
        let status = evaluate_threshold(8_192, 8_192, 1_024);
        assert_eq!(status, ThresholdStatus::Pass);
    }

    #[test]
    fn threshold_evaluation_warn_between_fail_and_warn() {
        let status = evaluate_threshold(4_096, 8_192, 1_024);
        assert_eq!(status, ThresholdStatus::Warn);
    }

    #[test]
    fn threshold_evaluation_fail_below_fail_threshold() {
        let status = evaluate_threshold(1_023, 8_192, 1_024);
        assert_eq!(status, ThresholdStatus::Fail);
    }
}
