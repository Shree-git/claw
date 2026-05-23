use std::path::Path;
use std::process::Command;

use clap::Args;
use serde::Serialize;

use claw_core::cof::{cof_decode, cof_encode};
use claw_core::object::Object;
use claw_core::types::Blob;
use claw_store::{ClawStore, HeadState};
use claw_sync::client::{RetryPolicy, SyncClient};
use claw_sync::protocol::{negotiated_protocol_version, SYNC_PROTOCOL_VERSION};
use claw_sync::transport::RemoteTransportConfig;

use crate::commands::remote::RemotesConfig;
use crate::config::find_repo_root;
use crate::{auth_store, commands::remote, config};

use super::repo_health::{inspect_ref_namespace, DeepHealthReport};

#[derive(Args)]
pub struct DoctorArgs {
    /// Output diagnostic report as JSON
    #[arg(long)]
    json: bool,
    /// Return a non-zero exit when any check reports an error
    #[arg(long)]
    strict: bool,
    /// Run a deep object graph, capsule, policy, and repairability scan
    #[arg(long)]
    deep: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Ok,
    Warning,
    Error,
    Skipped,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Warning => "warning",
            CheckStatus::Error => "error",
            CheckStatus::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCheck {
    name: &'static str,
    status: CheckStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<&'static str>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct DoctorSummary {
    ok: usize,
    warnings: usize,
    errors: usize,
    skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorReport {
    schema_version: u8,
    action: &'static str,
    version: &'static str,
    cwd: Option<String>,
    repo_root: Option<String>,
    checks: Vec<DoctorCheck>,
    summary: DoctorSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    deep: Option<DeepHealthReport>,
}

pub async fn run(args: DoctorArgs) -> anyhow::Result<()> {
    let report = build_report(args.deep).await;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }

    if args.strict && report.summary.errors > 0 {
        anyhow::bail!("doctor found {} error(s)", report.summary.errors);
    }

    Ok(())
}

async fn build_report(deep: bool) -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(check(
        "cli",
        CheckStatus::Ok,
        format!("claw {}", env!("CARGO_PKG_VERSION")),
        None,
    ));

    let cwd = match std::env::current_dir() {
        Ok(path) => {
            checks.push(check(
                "cwd",
                CheckStatus::Ok,
                path.display().to_string(),
                None,
            ));
            Some(path)
        }
        Err(err) => {
            checks.push(check(
                "cwd",
                CheckStatus::Error,
                format!("cannot read current directory: {err}"),
                Some("Check that the current directory still exists and is readable."),
            ));
            None
        }
    };

    add_git_check(&mut checks);
    add_object_format_check(&mut checks);

    let repo_root = match find_repo_root() {
        Ok(root) => {
            checks.push(check(
                "repository",
                CheckStatus::Ok,
                root.display().to_string(),
                None,
            ));
            Some(root)
        }
        Err(err) => {
            checks.push(check(
                "repository",
                CheckStatus::Warning,
                err.to_string(),
                Some("Run `claw init` here, or `cd` into an existing Claw repository."),
            ));
            None
        }
    };

    let mut deep_report = None;
    if let Some(root) = repo_root.as_deref() {
        add_layout_checks(root, &mut checks);
        add_config_check(root, &mut checks);
        add_store_checks(root, &mut checks);
        add_refs_validity_check(root, &mut checks);
        if deep {
            deep_report = add_deep_health_check(root, &mut checks);
        }
        add_remote_check(root, &mut checks);
        add_daemon_reachability_check(root, &mut checks).await;
        add_daemon_auth_check(root, &mut checks);
        add_writable_check(root, &mut checks);
    } else {
        for name in [
            "layout",
            "config",
            "head",
            "refs",
            "remotes",
            "daemon_reachable",
            "daemon_auth",
            "writable",
        ] {
            checks.push(check(
                name,
                CheckStatus::Skipped,
                "requires a Claw repository".to_string(),
                None,
            ));
        }
    }

    let summary = summarize(&checks);
    DoctorReport {
        schema_version: 1,
        action: "doctor",
        version: env!("CARGO_PKG_VERSION"),
        cwd: cwd.map(|path| path.display().to_string()),
        repo_root: repo_root.map(|path| path.display().to_string()),
        checks,
        summary,
        deep: deep_report,
    }
}

fn add_git_check(checks: &mut Vec<DoctorCheck>) {
    match Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            checks.push(check(
                "git",
                CheckStatus::Ok,
                if version.is_empty() {
                    "git is available".to_string()
                } else {
                    version
                },
                None,
            ));
        }
        Ok(output) => checks.push(check(
            "git",
            CheckStatus::Warning,
            format!("git exited with status {}", output.status),
            Some("Install Git or fix PATH before using git import/export workflows."),
        )),
        Err(err) => checks.push(check(
            "git",
            CheckStatus::Warning,
            format!("git not available: {err}"),
            Some("Install Git or fix PATH before using git import/export workflows."),
        )),
    }
}

fn add_object_format_check(checks: &mut Vec<DoctorCheck>) {
    let object = Object::Blob(Blob {
        data: b"doctor".to_vec(),
        media_type: Some("text/plain".to_string()),
    });

    let result: anyhow::Result<()> = (|| {
        let payload = object.serialize_payload()?;
        let encoded = cof_encode(object.type_tag(), &payload)?;
        let (decoded_type, decoded_payload) = cof_decode(&encoded)?;
        if decoded_type == object.type_tag() && decoded_payload == payload {
            Ok(())
        } else {
            anyhow::bail!("COF roundtrip mismatch")
        }
    })();

    match result {
        Ok(()) => checks.push(check(
            "object_format",
            CheckStatus::Ok,
            "COF v1 encode/decode supported".to_string(),
            None,
        )),
        Err(err) => checks.push(check(
            "object_format",
            CheckStatus::Error,
            format!("COF v1 roundtrip failed: {err}"),
            Some("Reinstall the CLI or rebuild from a clean checkout."),
        )),
    }
}

fn add_layout_checks(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let required = [
        root.join(".claw"),
        root.join(".claw").join("objects"),
        root.join(".claw").join("refs"),
        root.join(".claw").join("refs").join("heads"),
        root.join(".claw").join("HEAD"),
    ];

    let missing = required
        .iter()
        .filter(|path| !path.exists())
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect::<Vec<_>>();

    if missing.is_empty() {
        checks.push(check(
            "layout",
            CheckStatus::Ok,
            ".claw layout has required paths".to_string(),
            None,
        ));
    } else {
        checks.push(check(
            "layout",
            CheckStatus::Error,
            format!("missing {}", missing.join(", ")),
            Some("Back up the repository, then repair by restoring these paths or reinitializing with `claw init` in a clean directory."),
        ));
    }
}

fn add_config_check(root: &Path, checks: &mut Vec<DoctorCheck>) {
    match config::load_or_default_config(root) {
        Ok(_) => {
            let config_path = config::config_file_path(root);
            let message = if config_path.exists() {
                format!("loaded {}", config_path.display())
            } else {
                "no .claw/config.toml; using built-in defaults".to_string()
            };
            checks.push(check("config", CheckStatus::Ok, message, None));
        }
        Err(err) => checks.push(check(
            "config",
            CheckStatus::Error,
            err.to_string(),
            Some("Fix .claw/config.toml, or run `claw admin migrate apply --dry-run` to preview the current migration output."),
        )),
    }
}

fn add_store_checks(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let store = match ClawStore::open(root) {
        Ok(store) => store,
        Err(err) => {
            checks.push(check(
                "store",
                CheckStatus::Error,
                err.to_string(),
                Some("Confirm the .claw directory exists and is readable."),
            ));
            return;
        }
    };

    match store.read_head() {
        Ok(HeadState::Symbolic { ref_name }) => match store.resolve_head() {
            Ok(Some(id)) => checks.push(check(
                "head",
                CheckStatus::Ok,
                format!("{ref_name} -> {id}"),
                None,
            )),
            Ok(None) => checks.push(check(
                "head",
                CheckStatus::Warning,
                format!("{ref_name} has no commits yet"),
                Some("Create the first revision with `claw snapshot -m \"initial snapshot\"`."),
            )),
            Err(err) => checks.push(check(
                "head",
                CheckStatus::Error,
                err.to_string(),
                Some("Inspect .claw/HEAD and the matching file under .claw/refs."),
            )),
        },
        Ok(HeadState::Detached { target }) => checks.push(check(
            "head",
            CheckStatus::Ok,
            format!("detached at {target}"),
            None,
        )),
        Err(err) => checks.push(check(
            "head",
            CheckStatus::Error,
            err.to_string(),
            Some("Inspect .claw/HEAD and restore it to `ref: heads/main` if needed."),
        )),
    }
}

fn add_refs_validity_check(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let store = match ClawStore::open(root) {
        Ok(store) => store,
        Err(err) => {
            checks.push(check(
                "refs",
                CheckStatus::Skipped,
                format!("store unavailable: {err}"),
                None,
            ));
            return;
        }
    };

    let namespace_issues = inspect_ref_namespace(root);

    let refs = match store.list_refs("") {
        Ok(refs) => refs,
        Err(err) => {
            checks.push(check(
                "refs",
                CheckStatus::Error,
                err.to_string(),
                Some("Inspect .claw/refs for invalid names or object IDs."),
            ));
            return;
        }
    };

    let missing = refs
        .iter()
        .filter(|(_, id)| !store.has_object(id))
        .map(|(name, id)| format!("{name}->{id}"))
        .collect::<Vec<_>>();

    if namespace_issues.is_empty() && missing.is_empty() {
        checks.push(check(
            "refs",
            CheckStatus::Ok,
            format!("{} ref(s) point to local objects", refs.len()),
            None,
        ));
    } else {
        let mut messages = Vec::new();
        if !namespace_issues.is_empty() {
            messages.push(format!(
                "invalid ref namespace entries: {}",
                namespace_issues.join(", ")
            ));
        }
        if !missing.is_empty() {
            messages.push(format!("missing object(s): {}", missing.join(", ")));
        }
        checks.push(check(
            "refs",
            CheckStatus::Error,
            messages.join("; "),
            Some("Inspect .claw/refs, rename or remove non-portable refs, and restore missing objects from backup or repoint/delete invalid refs."),
        ));
    }
}

fn add_deep_health_check(root: &Path, checks: &mut Vec<DoctorCheck>) -> Option<DeepHealthReport> {
    let store = match ClawStore::open(root) {
        Ok(store) => store,
        Err(err) => {
            checks.push(check(
                "deep_health",
                CheckStatus::Skipped,
                format!("store unavailable: {err}"),
                None,
            ));
            return None;
        }
    };
    let report = super::repo_health::scan(&store);
    let status = if report.errors() > 0 {
        CheckStatus::Error
    } else if report.issue_count > 0 {
        CheckStatus::Warning
    } else {
        CheckStatus::Ok
    };
    checks.push(check(
        "deep_health",
        status,
        format!(
            "{} object(s), {} ref(s), {} issue(s), {} repairable",
            report.object_count, report.ref_count, report.issue_count, report.repairable_count
        ),
        if report.issue_count > 0 {
            Some("Run `claw repair plan --json` to inspect repairable issues before applying safe repairs.")
        } else {
            None
        },
    ));
    Some(report)
}

fn add_remote_check(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let config_path = root.join(".claw").join("remotes.toml");
    if !config_path.exists() {
        checks.push(check(
            "remotes",
            CheckStatus::Ok,
            "no remotes configured".to_string(),
            None,
        ));
        return;
    }

    match std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|content| toml::from_str::<RemotesConfig>(&content).ok())
    {
        Some(config) => checks.push(check(
            "remotes",
            CheckStatus::Ok,
            format!("{} remote(s) configured", config.remotes.len()),
            None,
        )),
        None => checks.push(check(
            "remotes",
            CheckStatus::Error,
            format!("invalid {}", config_path.display()),
            Some("Fix .claw/remotes.toml, or remove and recreate remotes with `claw remote add`."),
        )),
    }
}

async fn add_daemon_reachability_check(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let config_path = root.join(".claw").join("remotes.toml");
    if !config_path.exists() {
        checks.push(check(
            "daemon_reachable",
            CheckStatus::Skipped,
            "no remotes configured".to_string(),
            Some("Add a daemon remote with `claw remote add origin <url>` if this repository should sync."),
        ));
        return;
    }

    let config = match std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|content| toml::from_str::<RemotesConfig>(&content).ok())
    {
        Some(config) => config,
        None => {
            checks.push(check(
                "daemon_reachable",
                CheckStatus::Skipped,
                format!("invalid {}; remote reachability not checked", config_path.display()),
                Some("Fix .claw/remotes.toml, or remove and recreate remotes with `claw remote add`."),
            ));
            return;
        }
    };

    if config.remotes.is_empty() {
        checks.push(check(
            "daemon_reachable",
            CheckStatus::Skipped,
            "no remotes configured".to_string(),
            Some("Add a daemon remote with `claw remote add origin <url>` if this repository should sync."),
        ));
        return;
    }

    let mut messages = Vec::new();
    let mut has_warning = false;

    for name in config.remotes.keys() {
        match probe_remote(root, name).await {
            Ok(message) => messages.push(format!("{name}: {message}")),
            Err(err) => {
                has_warning = true;
                messages.push(format!("{name}: unreachable ({err})"));
            }
        }
    }

    checks.push(check(
        "daemon_reachable",
        if has_warning {
            CheckStatus::Warning
        } else {
            CheckStatus::Ok
        },
        messages.join("; "),
        if has_warning {
            Some("Start the configured daemon, fix the remote URL/TLS/auth settings, or remove stale remotes with `claw remote remove`.")
        } else {
            None
        },
    ));
}

async fn probe_remote(root: &Path, name: &str) -> anyhow::Result<String> {
    let resolved = remote::resolve_remote(root, name)?;
    let transport = match resolved {
        remote::ResolvedRemote::Grpc {
            addr,
            token_profile,
        } => RemoteTransportConfig::Grpc {
            addr,
            bearer_token: token_profile
                .as_deref()
                .map(|profile| auth_store::try_resolve_access_token(Some(profile)))
                .transpose()?
                .flatten(),
            tls: None,
        },
        remote::ResolvedRemote::ClawLab {
            base_url,
            repo,
            token_profile,
        } => RemoteTransportConfig::Http {
            base_url,
            repo,
            bearer_token: auth_store::try_resolve_access_token(token_profile.as_deref())?,
        },
    };

    let probe = async move {
        let mut client = SyncClient::connect_with_transport_and_retry(
            transport,
            RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        )
        .await?;
        let hello = client.hello().await?;
        let protocol = negotiated_protocol_version(&hello.capabilities)
            .map(str::to_string)
            .unwrap_or_else(|| "missing protocol marker".to_string());
        if protocol == SYNC_PROTOCOL_VERSION {
            Ok(format!(
                "reachable, server {}, protocol {}",
                hello.server_version, protocol
            ))
        } else {
            Ok(format!(
                "reachable, server {}, protocol {} (expected {})",
                hello.server_version, protocol, SYNC_PROTOCOL_VERSION
            ))
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(2), probe)
        .await
        .map_err(|_| anyhow::anyhow!("probe timed out after 2s"))?
}

fn add_daemon_auth_check(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let cfg = match config::load_or_default_config(root) {
        Ok(cfg) => cfg,
        Err(err) => {
            checks.push(check(
                "daemon_auth",
                CheckStatus::Skipped,
                format!("requires valid config: {err}"),
                None,
            ));
            return;
        }
    };

    let mut warnings = Vec::new();
    if cfg.auth.require_auth_for_daemon {
        let profile = config::default_profile(&cfg);
        match auth_store::try_resolve_access_token(Some(profile)) {
            Ok(Some(_)) => warnings.push(format!(
                "auth required; token profile '{profile}' is present"
            )),
            Ok(None) => warnings.push(format!(
                "auth required; token profile '{profile}' is missing"
            )),
            Err(err) => warnings.push(format!(
                "auth required; token profile '{profile}' could not be read: {err}"
            )),
        }
    } else {
        warnings.push("daemon auth is not required by config".to_string());
    }

    if cfg.tls.require_for_non_localhost {
        match (cfg.tls.cert_path.as_deref(), cfg.tls.key_path.as_deref()) {
            (Some(cert), Some(key))
                if config_path_exists(root, cert) && config_path_exists(root, key) =>
            {
                warnings.push("TLS required; cert/key paths exist".to_string());
            }
            (Some(_), Some(_)) => {
                warnings.push("TLS required; cert/key path missing".to_string());
            }
            _ => {
                warnings.push("TLS required for non-localhost; cert/key not configured".to_string())
            }
        }
    } else {
        warnings.push("TLS is not required for non-localhost by config".to_string());
    }

    let status = if warnings.iter().any(|message| {
        message.contains("missing")
            || message.contains("not configured")
            || message.contains("not required")
    }) {
        CheckStatus::Warning
    } else {
        CheckStatus::Ok
    };

    checks.push(check(
        "daemon_auth",
        status,
        warnings.join("; "),
        if status == CheckStatus::Warning {
            Some("For production daemons, configure auth tokens and TLS certificate/key paths before binding beyond localhost.")
        } else {
            None
        },
    ));
}

fn config_path_exists(root: &Path, value: &str) -> bool {
    let path = Path::new(value);
    if path.is_absolute() {
        path.exists()
    } else {
        root.join(path).exists()
    }
}

fn add_writable_check(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let claw_dir = root.join(".claw");
    match std::fs::metadata(&claw_dir) {
        Ok(metadata) if metadata.permissions().readonly() => checks.push(check(
            "writable",
            CheckStatus::Error,
            format!("{} is read-only", claw_dir.display()),
            Some(
                "Make the repository writable before running commands that update refs or objects.",
            ),
        )),
        Ok(_) => checks.push(check(
            "writable",
            CheckStatus::Ok,
            ".claw is writable by permission bits".to_string(),
            None,
        )),
        Err(err) => checks.push(check(
            "writable",
            CheckStatus::Error,
            err.to_string(),
            Some("Confirm the .claw directory exists and is readable."),
        )),
    }
}

fn check(
    name: &'static str,
    status: CheckStatus,
    message: String,
    remediation: Option<&'static str>,
) -> DoctorCheck {
    DoctorCheck {
        name,
        status,
        message,
        remediation,
    }
}

fn summarize(checks: &[DoctorCheck]) -> DoctorSummary {
    let mut summary = DoctorSummary::default();
    for check in checks {
        match check.status {
            CheckStatus::Ok => summary.ok += 1,
            CheckStatus::Warning => summary.warnings += 1,
            CheckStatus::Error => summary.errors += 1,
            CheckStatus::Skipped => summary.skipped += 1,
        }
    }
    summary
}

fn print_human(report: &DoctorReport) {
    println!("Claw doctor");
    for check in &report.checks {
        println!(
            "  {status:<8} {name:<12} {message}",
            status = check.status.label(),
            name = check.name,
            message = check.message
        );
        if let Some(remediation) = check.remediation {
            println!("           hint: {remediation}");
        }
    }
    println!(
        "Summary: {} ok, {} warning(s), {} error(s), {} skipped",
        report.summary.ok, report.summary.warnings, report.summary.errors, report.summary.skipped
    );
    if let Some(deep) = &report.deep {
        if !deep.issues.is_empty() {
            println!("Deep health issues:");
            for issue in &deep.issues {
                println!(
                    "  {severity:<8} {code:<24} {message}",
                    severity = issue.severity,
                    code = issue.code,
                    message = issue.message
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check, inspect_ref_namespace, summarize, CheckStatus, DoctorArgs};
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: DoctorArgs,
    }

    #[test]
    fn summary_counts_statuses() {
        let checks = vec![
            check("a", CheckStatus::Ok, "ok".to_string(), None),
            check("b", CheckStatus::Warning, "warn".to_string(), None),
            check("c", CheckStatus::Error, "err".to_string(), None),
            check("d", CheckStatus::Skipped, "skip".to_string(), None),
        ];

        let summary = summarize(&checks);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.warnings, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn parses_deep_flag() {
        let cli = TestCli::parse_from(["doctor", "--deep", "--strict"]);
        assert!(cli.args.deep);
        assert!(cli.args.strict);
    }

    #[test]
    fn ref_namespace_scan_reports_non_portable_ref_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let refs = tmp.path().join(".claw").join("refs");
        std::fs::create_dir_all(refs.join("heads")).expect("create refs");
        std::fs::write(refs.join("heads").join("CON"), b"not-an-id\n").expect("write ref");
        std::fs::write(refs.join("heads").join("trailing-dot."), b"not-an-id\n")
            .expect("write ref");
        let has_trailing_dot = std::fs::read_dir(refs.join("heads"))
            .expect("read refs")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == "trailing-dot.");

        let issues = inspect_ref_namespace(tmp.path());

        assert!(
            issues.iter().any(|issue| issue.contains("heads/CON")),
            "issues: {issues:#?}"
        );
        if has_trailing_dot {
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.contains("heads/trailing-dot.")),
                "issues: {issues:#?}"
            );
        }
    }

    #[test]
    fn ref_namespace_scan_reports_case_collisions_when_filesystem_allows_them() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let refs = tmp.path().join(".claw").join("refs").join("heads");
        std::fs::create_dir_all(&refs).expect("create refs");
        std::fs::write(refs.join("main"), b"not-an-id\n").expect("write ref");
        if std::fs::write(refs.join("MAIN"), b"not-an-id\n").is_err() {
            return;
        }
        let names = std::fs::read_dir(&refs)
            .expect("read refs")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>();
        if !names.iter().any(|name| name == "main") || !names.iter().any(|name| name == "MAIN") {
            return;
        }

        let issues = inspect_ref_namespace(tmp.path());

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("case-insensitive collision")),
            "issues: {issues:#?}"
        );
    }
}
