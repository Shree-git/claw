use std::collections::BTreeMap;
use std::path::Path;

use clap::{Args, Subcommand};

use crate::config::find_repo_root;

#[derive(Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    command: RemoteCommand,
}

#[derive(Subcommand)]
enum RemoteCommand {
    /// Add a remote
    Add {
        /// Remote name
        name: String,
        /// URL for gRPC remotes or base URL for clawlab remotes
        url: String,
        /// Transport kind (grpc|clawlab)
        #[arg(long, default_value = "grpc")]
        kind: String,
        /// Repository slug for clawlab remotes
        #[arg(long)]
        repo: Option<String>,
        /// Auth profile for clawlab remotes
        #[arg(long)]
        token_profile: Option<String>,
    },
    /// List remotes
    List,
    /// Remove a remote
    Remove {
        /// Remote name
        name: String,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct RemotesConfig {
    #[serde(default)]
    pub remotes: BTreeMap<String, RemoteEntry>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub(crate) struct RemoteEntry {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub token_profile: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ResolvedRemote {
    Grpc {
        addr: String,
    },
    ClawLab {
        base_url: String,
        repo: String,
        token_profile: Option<String>,
    },
}

pub fn run(args: RemoteArgs) -> anyhow::Result<()> {
    match args.command {
        RemoteCommand::Add {
            name,
            url,
            kind,
            repo,
            token_profile,
        } => run_add(&name, &url, &kind, repo, token_profile),
        RemoteCommand::List => run_list(),
        RemoteCommand::Remove { name } => run_remove(&name),
    }
}

fn run_add(
    name: &str,
    url: &str,
    kind: &str,
    repo: Option<String>,
    token_profile: Option<String>,
) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let config_path = root.join(".claw").join("remotes.toml");

    let mut config = load_remotes(&config_path);
    if config.remotes.contains_key(name) {
        anyhow::bail!("remote '{}' already exists", name);
    }

    let entry = match kind {
        "grpc" => RemoteEntry {
            kind: Some("grpc".to_string()),
            url: Some(url.to_string()),
            ..RemoteEntry::default()
        },
        "clawlab" => {
            let repo = repo.ok_or_else(|| {
                anyhow::anyhow!("--repo is required for clawlab remotes (example: acme-repo)")
            })?;
            RemoteEntry {
                kind: Some("clawlab".to_string()),
                base_url: Some(url.to_string()),
                repo: Some(repo),
                token_profile,
                ..RemoteEntry::default()
            }
        }
        other => anyhow::bail!("unsupported remote kind: {other} (expected grpc|clawlab)"),
    };

    config.remotes.insert(name.to_string(), entry);
    save_remotes(&config_path, &config)?;

    println!("Added remote '{}' ({kind}) -> {url}", name);
    Ok(())
}

fn run_list() -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let config_path = root.join(".claw").join("remotes.toml");

    let config = load_remotes(&config_path);
    if config.remotes.is_empty() {
        println!("No remotes configured.");
        return Ok(());
    }

    for (name, entry) in &config.remotes {
        match normalize_entry(entry.clone())? {
            ResolvedRemote::Grpc { addr } => {
                println!("{}\tgrpc\t{}", name, addr);
            }
            ResolvedRemote::ClawLab {
                base_url,
                repo,
                token_profile,
            } => {
                println!(
                    "{}\tclawlab\t{}\t{}\t{}",
                    name,
                    base_url,
                    repo,
                    token_profile.unwrap_or_else(|| "default".to_string())
                );
            }
        }
    }
    Ok(())
}

fn run_remove(name: &str) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let config_path = root.join(".claw").join("remotes.toml");

    let mut config = load_remotes(&config_path);
    if config.remotes.remove(name).is_none() {
        anyhow::bail!("remote '{}' not found", name);
    }
    save_remotes(&config_path, &config)?;

    println!("Removed remote '{}'", name);
    Ok(())
}

pub fn load_remotes(config_path: &Path) -> RemotesConfig {
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            if let Ok(config) = toml::from_str(&content) {
                return config;
            }
        }
    }
    RemotesConfig::default()
}

fn save_remotes(config_path: &Path, config: &RemotesConfig) -> anyhow::Result<()> {
    let content = toml::to_string_pretty(config)?;
    std::fs::write(config_path, content)?;
    Ok(())
}

fn normalize_entry(entry: RemoteEntry) -> anyhow::Result<ResolvedRemote> {
    let kind = entry
        .kind
        .clone()
        .or_else(|| {
            if entry.base_url.is_some() || entry.repo.is_some() {
                Some("clawlab".to_string())
            } else {
                Some("grpc".to_string())
            }
        })
        .unwrap_or_else(|| "grpc".to_string());

    match kind.as_str() {
        "grpc" => {
            let addr = entry
                .url
                .or(entry.base_url)
                .ok_or_else(|| anyhow::anyhow!("missing grpc url in remote entry"))?;
            Ok(ResolvedRemote::Grpc { addr })
        }
        "clawlab" => {
            let base_url = entry
                .base_url
                .or(entry.url)
                .ok_or_else(|| anyhow::anyhow!("missing base_url in clawlab remote entry"))?;
            let repo = entry
                .repo
                .ok_or_else(|| anyhow::anyhow!("missing repo in clawlab remote entry"))?;
            Ok(ResolvedRemote::ClawLab {
                base_url,
                repo,
                token_profile: entry.token_profile,
            })
        }
        other => anyhow::bail!("unsupported remote kind in config: {other}"),
    }
}

/// Resolve a remote argument to its transport-specific connection details.
pub fn resolve_remote(root: &Path, remote_arg: &str) -> anyhow::Result<ResolvedRemote> {
    if remote_arg.contains("://") || remote_arg.contains("localhost") {
        return Ok(ResolvedRemote::Grpc {
            addr: remote_arg.to_string(),
        });
    }

    let config_path = root.join(".claw").join("remotes.toml");
    let config = load_remotes(&config_path);
    let entry = config.remotes.get(remote_arg).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "remote '{}' not found. Use a URL or `claw remote add`.",
            remote_arg
        )
    })?;

    normalize_entry(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_legacy_grpc_entry() {
        let entry = RemoteEntry {
            url: Some("http://localhost:50051".to_string()),
            ..RemoteEntry::default()
        };

        match normalize_entry(entry).unwrap() {
            ResolvedRemote::Grpc { addr } => assert_eq!(addr, "http://localhost:50051"),
            _ => panic!("expected grpc"),
        }
    }

    #[test]
    fn normalize_clawlab_entry() {
        let entry = RemoteEntry {
            kind: Some("clawlab".to_string()),
            base_url: Some("https://api.clawlab.com".to_string()),
            repo: Some("acme-repo".to_string()),
            token_profile: Some("default".to_string()),
            ..RemoteEntry::default()
        };

        match normalize_entry(entry).unwrap() {
            ResolvedRemote::ClawLab {
                base_url,
                repo,
                token_profile,
            } => {
                assert_eq!(base_url, "https://api.clawlab.com");
                assert_eq!(repo, "acme-repo");
                assert_eq!(token_profile.as_deref(), Some("default"));
            }
            _ => panic!("expected clawlab"),
        }
    }
}
