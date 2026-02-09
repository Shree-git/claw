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
        /// Remote URL
        url: String,
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct RemoteEntry {
    pub url: String,
}

pub fn run(args: RemoteArgs) -> anyhow::Result<()> {
    match args.command {
        RemoteCommand::Add { name, url } => run_add(&name, &url),
        RemoteCommand::List => run_list(),
        RemoteCommand::Remove { name } => run_remove(&name),
    }
}

fn run_add(name: &str, url: &str) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let config_path = root.join(".claw").join("remotes.toml");

    let mut config = load_remotes(&config_path);
    if config.remotes.contains_key(name) {
        anyhow::bail!("remote '{}' already exists", name);
    }
    config.remotes.insert(
        name.to_string(),
        RemoteEntry {
            url: url.to_string(),
        },
    );
    save_remotes(&config_path, &config)?;

    println!("Added remote '{}' -> {}", name, url);
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
        println!("{}\t{}", name, entry.url);
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

/// Resolve a remote name to its URL. If the input looks like a URL already, return it as-is.
pub fn resolve_remote_url(root: &Path, remote_arg: &str) -> anyhow::Result<String> {
    // If it looks like a URL, use it directly
    if remote_arg.contains("://") || remote_arg.contains("localhost") {
        return Ok(remote_arg.to_string());
    }

    let config_path = root.join(".claw").join("remotes.toml");
    let config = load_remotes(&config_path);
    config
        .remotes
        .get(remote_arg)
        .map(|e| e.url.clone())
        .ok_or_else(|| anyhow::anyhow!("remote '{}' not found. Use a URL or `claw remote add`.", remote_arg))
}
