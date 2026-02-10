use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct AuthConfig {
    #[serde(default)]
    pub profiles: BTreeMap<String, AuthProfile>,
}

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct AuthProfile {
    pub base_url: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at_unix: Option<u64>,
}

fn home_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not find home directory"))
}

pub fn auth_config_path() -> anyhow::Result<PathBuf> {
    let path = home_dir()?.join(".claw").join("auth.toml");
    Ok(path)
}

pub fn load_auth_config() -> AuthConfig {
    let path = match auth_config_path() {
        Ok(p) => p,
        Err(_) => return AuthConfig::default(),
    };

    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(config) = toml::from_str(&content) {
                return config;
            }
        }
    }

    AuthConfig::default()
}

pub fn save_auth_config(config: &AuthConfig) -> anyhow::Result<()> {
    let path = auth_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn resolve_access_token(profile: Option<&str>) -> Option<String> {
    let profile = profile.unwrap_or("default");
    let config = load_auth_config();
    config.profiles.get(profile).map(|p| p.access_token.clone())
}
