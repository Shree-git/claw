use std::io::{self, Write};

use base64::prelude::*;
use clap::{Args, Subcommand};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::auth_store::{load_auth_config, save_auth_config, AuthProfile};

#[derive(Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Login to ClawLab with browser PKCE flow
    Login {
        /// Base URL of ClawLab API
        #[arg(long, default_value = "https://api.clawlab.com")]
        base_url: String,
        /// Auth profile name
        #[arg(long, default_value = "default")]
        profile: String,
        /// Do not open browser automatically
        #[arg(long)]
        no_browser: bool,
    },
    /// Logout from a saved profile
    Logout {
        /// Auth profile name
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Manage tokens
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },
}

#[derive(Subcommand)]
enum TokenCommand {
    /// Set access token manually
    Set {
        token: String,
        #[arg(long, default_value = "https://api.clawlab.com")]
        base_url: String,
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// Show token metadata for a profile
    Show {
        #[arg(long, default_value = "default")]
        profile: String,
    },
    /// List configured profiles
    List,
}

#[derive(serde::Serialize)]
struct TokenRequest {
    grant_type: String,
    client_id: String,
    code: String,
    code_verifier: String,
    redirect_uri: String,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

fn random_urlsafe(n: usize) -> String {
    let mut bytes = vec![0_u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    BASE64_URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn prompt_input(prompt: &str) -> anyhow::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

pub async fn run(args: AuthArgs) -> anyhow::Result<()> {
    match args.command {
        AuthCommand::Login {
            base_url,
            profile,
            no_browser,
        } => login(base_url, profile, no_browser).await,
        AuthCommand::Logout { profile } => logout(profile),
        AuthCommand::Token { command } => token(command),
    }
}

async fn login(base_url: String, profile: String, no_browser: bool) -> anyhow::Result<()> {
    let verifier = random_urlsafe(48);
    let challenge = pkce_challenge(&verifier);
    let state = random_urlsafe(16);
    let redirect_uri = "urn:ietf:wg:oauth:2.0:oob";

    let authorize_url = format!(
        "{}/oauth/authorize?response_type=code&client_id=claw-cli&code_challenge_method=S256&code_challenge={}&redirect_uri={}&state={}",
        base_url.trim_end_matches('/'),
        urlencoding::encode(&challenge),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&state)
    );

    if !no_browser {
        let _ = webbrowser::open(&authorize_url);
    }

    println!("Open this URL to authenticate:\n{authorize_url}\n");
    let code = prompt_input("Paste authorization code: ")?;
    if code.is_empty() {
        anyhow::bail!("authorization code is required");
    }

    let client = reqwest::Client::new();
    let token_url = format!("{}/oauth/token", base_url.trim_end_matches('/'));
    let body = TokenRequest {
        grant_type: "authorization_code".to_string(),
        client_id: "claw-cli".to_string(),
        code,
        code_verifier: verifier,
        redirect_uri: redirect_uri.to_string(),
    };

    let response = client.post(&token_url).json(&body).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "token exchange failed ({}). You can fallback to `claw auth token set <token> --profile {}`. Body: {}",
            status,
            profile,
            text
        );
    }

    let token_response: TokenResponse = response.json().await?;
    let expires_at_unix = token_response
        .expires_in
        .map(|seconds| std::time::SystemTime::now() + std::time::Duration::from_secs(seconds))
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|dur| dur.as_secs());

    let mut config = load_auth_config();
    config.profiles.insert(
        profile.clone(),
        AuthProfile {
            base_url,
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at_unix,
        },
    );
    save_auth_config(&config)?;

    println!("Saved auth profile '{profile}'");
    Ok(())
}

fn logout(profile: String) -> anyhow::Result<()> {
    let mut config = load_auth_config();
    if config.profiles.remove(&profile).is_none() {
        anyhow::bail!("profile '{}' not found", profile);
    }

    save_auth_config(&config)?;
    println!("Logged out profile '{profile}'");
    Ok(())
}

fn token(command: TokenCommand) -> anyhow::Result<()> {
    match command {
        TokenCommand::Set {
            token,
            base_url,
            profile,
        } => {
            let mut config = load_auth_config();
            config.profiles.insert(
                profile.clone(),
                AuthProfile {
                    base_url,
                    access_token: token,
                    refresh_token: None,
                    expires_at_unix: None,
                },
            );
            save_auth_config(&config)?;
            println!("Stored token in profile '{profile}'");
        }
        TokenCommand::Show { profile } => {
            let config = load_auth_config();
            let entry = config
                .profiles
                .get(&profile)
                .ok_or_else(|| anyhow::anyhow!("profile '{}' not found", profile))?;

            let masked = if entry.access_token.len() > 10 {
                format!("{}...", &entry.access_token[0..10])
            } else {
                "***".to_string()
            };

            println!("profile: {profile}");
            println!("base_url: {}", entry.base_url);
            println!("access_token: {masked}");
            if let Some(exp) = entry.expires_at_unix {
                println!("expires_at_unix: {exp}");
            }
        }
        TokenCommand::List => {
            let config = load_auth_config();
            if config.profiles.is_empty() {
                println!("No auth profiles configured");
            } else {
                for (name, profile) in config.profiles {
                    println!("{}\t{}", name, profile.base_url);
                }
            }
        }
    }

    Ok(())
}
