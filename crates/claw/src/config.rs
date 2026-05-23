use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff_render;

const SUPPORTED_CONFIG_VERSION: u32 = 1;

#[derive(Debug)]
pub struct NotRepositoryError;

impl std::fmt::Display for NotRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not in a claw repository (no .claw directory found)")
    }
}

impl std::error::Error for NotRepositoryError {}

/// Find the claw repo root by walking up from the current directory.
pub fn find_repo_root() -> anyhow::Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if is_repository_metadata_dir(&dir.join(".claw")) {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(NotRepositoryError.into());
        }
    }
}

fn is_repository_metadata_dir(claw_dir: &Path) -> bool {
    if !claw_dir.is_dir() {
        return false;
    }

    claw_dir.join("config.toml").is_file()
        || claw_dir.join("HEAD").is_file()
        || claw_dir.join("objects").is_dir()
        || claw_dir.join("refs").is_dir()
}

pub fn config_file_path(root: &Path) -> PathBuf {
    root.join(".claw").join("config.toml")
}

fn ensure_config_version_compatible(current: u32, target: u32) -> anyhow::Result<()> {
    if current == target {
        return Ok(());
    }

    if current > target {
        anyhow::bail!(
            "unsupported config_version={} (latest supported is {})",
            current,
            target
        );
    }

    anyhow::bail!(
        "unsupported config_version={} (expected {})",
        current,
        target
    );
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClawConfigV1 {
    pub config_version: u32,
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub tls: TlsSection,
    #[serde(default)]
    pub timeouts: TimeoutSection,
    #[serde(default)]
    pub retries: RetrySection,
    #[serde(default)]
    pub queues: QueueSection,
    #[serde(default)]
    pub telemetry: TelemetrySection,
    #[serde(default)]
    pub policy: PolicySection,
    #[serde(default)]
    pub backup: BackupSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthSection {
    pub require_auth_for_daemon: bool,
    pub default_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsSection {
    pub require_for_non_localhost: bool,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutSection {
    pub io_ms: u64,
    pub git_bridge_ms: u64,
    pub policy_eval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrySection {
    pub idempotent_only: bool,
    pub max_attempts: u32,
    pub base_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub jitter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueSection {
    pub worker_pool_size: usize,
    pub queue_capacity: usize,
    pub backpressure: bool,
    pub rate_limit_per_minute: Option<u32>,
    pub max_push_chunk_bytes: Option<usize>,
    pub max_push_request_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetrySection {
    pub structured_logs: bool,
    pub correlation_ids: bool,
    pub metrics: bool,
    pub traces: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicySection {
    pub fail_closed_integrate: bool,
    pub fail_closed_ship: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSection {
    pub snapshot_interval_min: u64,
    pub verify_integrity_on_startup: bool,
    pub strict_startup_checks: bool,
}

impl Default for ClawConfigV1 {
    fn default() -> Self {
        Self {
            config_version: SUPPORTED_CONFIG_VERSION,
            auth: AuthSection::default(),
            tls: TlsSection::default(),
            timeouts: TimeoutSection::default(),
            retries: RetrySection::default(),
            queues: QueueSection::default(),
            telemetry: TelemetrySection::default(),
            policy: PolicySection::default(),
            backup: BackupSection::default(),
        }
    }
}

impl Default for AuthSection {
    fn default() -> Self {
        Self {
            require_auth_for_daemon: true,
            default_profile: "default".to_string(),
        }
    }
}

impl Default for TlsSection {
    fn default() -> Self {
        Self {
            require_for_non_localhost: true,
            cert_path: None,
            key_path: None,
        }
    }
}

impl Default for TimeoutSection {
    fn default() -> Self {
        Self {
            io_ms: 10_000,
            git_bridge_ms: 15_000,
            policy_eval_ms: 5_000,
        }
    }
}

impl Default for RetrySection {
    fn default() -> Self {
        Self {
            idempotent_only: true,
            max_attempts: 4,
            base_backoff_ms: 100,
            max_backoff_ms: 2_000,
            jitter: true,
        }
    }
}

impl Default for QueueSection {
    fn default() -> Self {
        Self {
            worker_pool_size: 8,
            queue_capacity: 1_024,
            backpressure: true,
            rate_limit_per_minute: None,
            max_push_chunk_bytes: Some(8 * 1024 * 1024),
            max_push_request_bytes: Some(128 * 1024 * 1024),
        }
    }
}

impl Default for TelemetrySection {
    fn default() -> Self {
        Self {
            structured_logs: true,
            correlation_ids: true,
            metrics: true,
            traces: true,
        }
    }
}

impl Default for PolicySection {
    fn default() -> Self {
        Self {
            fail_closed_integrate: true,
            fail_closed_ship: true,
        }
    }
}

impl Default for BackupSection {
    fn default() -> Self {
        Self {
            snapshot_interval_min: 60,
            verify_integrity_on_startup: true,
            strict_startup_checks: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigMigrationPlan {
    pub source: Option<PathBuf>,
    pub next_content: String,
    pub diff: String,
    pub target: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LegacyRepoConfig {
    version: Option<u32>,
    name: Option<String>,
}

pub fn load_or_default_config(root: &Path) -> anyhow::Result<ClawConfigV1> {
    let path = config_file_path(root);
    if !path.exists() {
        return Ok(ClawConfigV1::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let parsed: ClawConfigV1 = toml::from_str(&content)
        .map_err(|err| anyhow::anyhow!("invalid config {}: {err}", path.display()))?;

    ensure_config_version_compatible(parsed.config_version, SUPPORTED_CONFIG_VERSION)
        .map_err(|err| anyhow::anyhow!("{} in {}", err, path.display()))?;

    Ok(parsed)
}

#[allow(dead_code)]
pub fn save_config(root: &Path, config: &ClawConfigV1) -> anyhow::Result<()> {
    let path = config_file_path(root);
    let content = toml::to_string_pretty(config)?;
    atomic_write(&path, content.as_bytes())?;
    Ok(())
}

pub fn plan_config_migration(root: &Path) -> anyhow::Result<ConfigMigrationPlan> {
    let target = config_file_path(root);
    let legacy_path = root.join(".claw").join("repo.toml");

    if target.exists() {
        let current = std::fs::read_to_string(&target)?;
        let config: ClawConfigV1 = toml::from_str(&current)
            .map_err(|err| anyhow::anyhow!("invalid config {}: {err}", target.display()))?;
        ensure_config_version_compatible(config.config_version, SUPPORTED_CONFIG_VERSION)
            .map_err(|err| anyhow::anyhow!("{} in {}", err, target.display()))?;
        let next = toml::to_string_pretty(&config)?;
        let diff = diff_render::render_unified_diff(
            ".claw/config.toml",
            current.as_bytes(),
            next.as_bytes(),
        );

        return Ok(ConfigMigrationPlan {
            source: Some(target.clone()),
            next_content: next,
            diff,
            target,
        });
    }

    let (source, current_content, legacy): (Option<PathBuf>, String, LegacyRepoConfig) =
        if legacy_path.exists() {
            let content = std::fs::read_to_string(&legacy_path)?;
            let parsed = toml::from_str::<LegacyRepoConfig>(&content).map_err(|err| {
                anyhow::anyhow!("invalid legacy config {}: {err}", legacy_path.display())
            })?;
            (Some(legacy_path), content, parsed)
        } else {
            (None, String::new(), LegacyRepoConfig::default())
        };

    let mut next_config = ClawConfigV1::default();
    if let Some(version) = legacy.version {
        tracing::debug!(
            "legacy repo config version={} migrated to config_version=1",
            version
        );
    }
    if let Some(name) = legacy.name {
        if !name.trim().is_empty() {
            next_config.auth.default_profile = "default".to_string();
        }
    }

    let next_content = toml::to_string_pretty(&next_config)?;
    let diff = diff_render::render_unified_diff(
        ".claw/config.toml",
        current_content.as_bytes(),
        next_content.as_bytes(),
    );

    Ok(ConfigMigrationPlan {
        source,
        next_content,
        diff,
        target,
    })
}

pub fn apply_config_migration(root: &Path) -> anyhow::Result<ConfigMigrationPlan> {
    let plan = plan_config_migration(root)?;
    atomic_write(&plan.target, plan.next_content.as_bytes())?;
    Ok(plan)
}

fn atomic_write(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(content)?;
        file.sync_all()?;
    }
    temp.persist(path).map_err(|err| {
        anyhow::anyhow!(
            "failed to replace config file {}: {}",
            path.display(),
            err.error
        )
    })?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

pub fn default_profile(config: &ClawConfigV1) -> &str {
    let profile = config.auth.default_profile.trim();
    if profile.is_empty() {
        "default"
    } else {
        profile
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();

        let expected = ClawConfigV1::default();
        save_config(root, &expected).expect("save default config");

        let loaded = load_or_default_config(root).expect("load saved config");
        assert_eq!(loaded, expected);
    }

    #[test]
    fn repository_discovery_ignores_auth_only_claw_dirs() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let claw_dir = temp.path().join(".claw");
        fs::create_dir_all(&claw_dir).expect("create .claw dir");
        fs::write(claw_dir.join("auth.toml"), "[profiles]\n").expect("write auth config");

        assert!(!is_repository_metadata_dir(&claw_dir));

        fs::create_dir_all(claw_dir.join("objects")).expect("create object store");
        assert!(is_repository_metadata_dir(&claw_dir));
    }

    #[test]
    fn migration_plan_generation_from_legacy_repo_toml() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        let claw_dir = root.join(".claw");
        fs::create_dir_all(&claw_dir).expect("create .claw dir");

        fs::write(
            claw_dir.join("repo.toml"),
            "version = 7\nname = \"sample\"\n",
        )
        .expect("write legacy repo.toml");

        let plan = plan_config_migration(root).expect("build migration plan");

        assert_eq!(plan.source, Some(claw_dir.join("repo.toml")));
        assert_eq!(plan.target, claw_dir.join("config.toml"));
        assert!(plan.next_content.contains("config_version = 1"));
        assert!(plan.diff.contains(".claw/config.toml"));
    }

    #[test]
    fn apply_config_migration_writes_config_atomically() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        let claw_dir = root.join(".claw");
        fs::create_dir_all(&claw_dir).expect("create .claw dir");
        fs::write(claw_dir.join("repo.toml"), "version = 7\n").expect("write legacy repo.toml");

        let applied = apply_config_migration(root).expect("apply config migration");
        assert_eq!(applied.target, claw_dir.join("config.toml"));

        let loaded = load_or_default_config(root).expect("load migrated config");
        assert_eq!(loaded.config_version, SUPPORTED_CONFIG_VERSION);

        let temp_entries = fs::read_dir(&claw_dir)
            .expect("read .claw dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
            .count();
        assert_eq!(
            temp_entries, 0,
            "atomic config write should not leave temp files"
        );
    }

    #[test]
    fn rejects_unsupported_config_version() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let root = temp.path();
        let claw_dir = root.join(".claw");
        fs::create_dir_all(&claw_dir).expect("create .claw dir");

        fs::write(claw_dir.join("config.toml"), "config_version = 2\n")
            .expect("write unsupported config");

        let err = load_or_default_config(root)
            .expect_err("config version 2 should be rejected")
            .to_string();
        assert!(err.contains("unsupported config_version=2"));
    }

    #[test]
    fn compatibility_allows_same_version() {
        let result =
            ensure_config_version_compatible(SUPPORTED_CONFIG_VERSION, SUPPORTED_CONFIG_VERSION);
        assert!(result.is_ok());
    }

    #[test]
    fn compatibility_rejects_future_version() {
        let err = ensure_config_version_compatible(
            SUPPORTED_CONFIG_VERSION + 1,
            SUPPORTED_CONFIG_VERSION,
        )
        .expect_err("future version should be rejected")
        .to_string();
        assert!(err.contains("unsupported config_version=2"));
        assert!(err.contains("latest supported is 1"));
    }

    #[test]
    fn compatibility_rejects_older_version_without_migration() {
        let err = ensure_config_version_compatible(0, SUPPORTED_CONFIG_VERSION)
            .expect_err("older version should be rejected")
            .to_string();
        assert!(err.contains("unsupported config_version=0"));
        assert!(err.contains("expected 1"));
    }
}
