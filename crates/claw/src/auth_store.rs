use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::prelude::*;
use rand::RngCore;

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

const TOKEN_ENCRYPTION_PREFIX: &str = "enc:v1:";

fn home_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not find home directory"))
}

fn claw_home_dir() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".claw"))
}

pub fn auth_config_path() -> anyhow::Result<PathBuf> {
    let path = claw_home_dir()?.join("auth.toml");
    Ok(path)
}

fn auth_key_path() -> anyhow::Result<PathBuf> {
    Ok(claw_home_dir()?.join("auth.key"))
}

fn set_private_permissions(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(not(unix))]
    let _ = path;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn load_or_create_auth_key() -> anyhow::Result<[u8; 32]> {
    let path = auth_key_path()?;
    if path.exists() {
        let bytes = std::fs::read(&path)?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid auth key length; expected 32 bytes"))?;
        set_private_permissions(&path)?;
        return Ok(arr);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    atomic_write_private(&path, &key)?;
    set_private_permissions(&path)?;
    Ok(key)
}

fn encrypt_token(token: &str) -> anyhow::Result<String> {
    let key = load_or_create_auth_key()?;
    let encrypted = claw_crypto::encrypt::encrypt(&key, token.as_bytes())?;
    Ok(format!(
        "{TOKEN_ENCRYPTION_PREFIX}{}",
        BASE64_STANDARD.encode(encrypted)
    ))
}

fn decrypt_token(token: &str) -> anyhow::Result<String> {
    let Some(payload) = token.strip_prefix(TOKEN_ENCRYPTION_PREFIX) else {
        return Ok(token.to_string());
    };

    let key = load_or_create_auth_key()?;
    let encrypted = BASE64_STANDARD
        .decode(payload)
        .map_err(|e| anyhow::anyhow!("invalid encrypted token format: {e}"))?;
    let decrypted = claw_crypto::encrypt::decrypt(&key, &encrypted)?;
    String::from_utf8(decrypted).map_err(|e| anyhow::anyhow!("decrypted token is not utf-8: {e}"))
}

fn decrypt_profile_tokens(profile_name: &str, profile: &mut AuthProfile) {
    match decrypt_token(&profile.access_token) {
        Ok(token) => profile.access_token = token,
        Err(err) => {
            tracing::warn!(
                "failed to decrypt access token for profile '{}': {}",
                profile_name,
                err
            );
            profile.access_token.clear();
        }
    }

    let refresh = profile.refresh_token.clone();
    profile.refresh_token = refresh.and_then(|token| match decrypt_token(&token) {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::warn!(
                "failed to decrypt refresh token for profile '{}': {}",
                profile_name,
                err
            );
            None
        }
    });
}

pub fn try_load_auth_config() -> anyhow::Result<AuthConfig> {
    let path = auth_config_path()?;

    if !path.exists() {
        return Ok(AuthConfig::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|err| anyhow::anyhow!("failed to read auth config {}: {err}", path.display()))?;
    let mut config = toml::from_str::<AuthConfig>(&content)
        .map_err(|err| anyhow::anyhow!("invalid auth config {}: {err}", path.display()))?;
    for (profile_name, profile) in &mut config.profiles {
        decrypt_profile_tokens(profile_name, profile);
    }
    Ok(config)
}

pub fn save_auth_config(config: &AuthConfig) -> anyhow::Result<()> {
    let path = auth_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut persisted = config.clone();
    for profile in persisted.profiles.values_mut() {
        profile.access_token = encrypt_token(&profile.access_token)?;
        profile.refresh_token = profile
            .refresh_token
            .as_ref()
            .map(|token| encrypt_token(token))
            .transpose()?;
    }

    let content = toml::to_string_pretty(&persisted)?;
    atomic_write_private(&path, content.as_bytes())?;
    set_private_permissions(&auth_config_path()?)?;
    Ok(())
}

fn atomic_write_private(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("auth path has no parent: {}", path.display()))?;
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
            "failed to replace auth file {}: {}",
            path.display(),
            err.error
        )
    })?;
    set_private_permissions(path)?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

pub fn try_resolve_access_token(profile: Option<&str>) -> anyhow::Result<Option<String>> {
    let profile = profile.unwrap_or("default");
    let config = try_load_auth_config()?;
    Ok(config.profiles.get(profile).map(|p| p.access_token.clone()))
}
