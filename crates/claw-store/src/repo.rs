use serde::{Deserialize, Serialize};

use crate::layout::RepoLayout;
use crate::StoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub version: u32,
    pub name: Option<String>,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            version: 1,
            name: None,
        }
    }
}

pub fn write_default_config(layout: &RepoLayout) -> Result<(), StoreError> {
    let config = RepoConfig::default();
    let toml_str =
        toml::to_string_pretty(&config).map_err(|e| StoreError::Config(e.to_string()))?;
    std::fs::write(layout.config_file(), toml_str)?;
    Ok(())
}

pub fn read_config(layout: &RepoLayout) -> Result<RepoConfig, StoreError> {
    let content = std::fs::read_to_string(layout.config_file())?;
    let config: RepoConfig =
        toml::from_str(&content).map_err(|e| StoreError::Config(e.to_string()))?;
    Ok(config)
}
