use std::io::{self, Read, Write};

use base64::prelude::*;
use clap::{Args, Subcommand};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::auth_store::{save_auth_config, try_load_auth_config, AuthProfile};

// Hosted auth endpoints use this public CLI client id by convention.
const HOSTED_OAUTH_CLIENT_ID: &str = "claw-cli";

#[derive(Args)]
pub struct AuthArgs {
    /// Output command results as JSON without printing token secrets
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Login to a configured hosted remote with browser PKCE flow
    Login {
        /// Base URL of the hosted auth API
        #[arg(long, value_name = "URL")]
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
        /// Access token. Prefer --stdin to avoid shell history exposure.
        token: Option<String>,
        /// Read the access token from stdin instead of a command argument.
        #[arg(long)]
        stdin: bool,
        /// Base URL of the hosted auth API
        #[arg(long, value_name = "URL")]
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

fn prompt_input(prompt: &str, stderr: bool) -> anyhow::Result<String> {
    if stderr {
        eprint!("{prompt}");
        io::stderr().flush()?;
    } else {
        print!("{prompt}");
        io::stdout().flush()?;
    }

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
        } => login(base_url, profile, no_browser, args.json).await,
        AuthCommand::Logout { profile } => logout(profile, args.json),
        AuthCommand::Token { command } => token(command, args.json),
    }
}

async fn login(
    base_url: String,
    profile: String,
    no_browser: bool,
    json: bool,
) -> anyhow::Result<()> {
    let verifier = random_urlsafe(48);
    let challenge = pkce_challenge(&verifier);
    let state = random_urlsafe(16);
    let redirect_uri = "urn:ietf:wg:oauth:2.0:oob";

    let authorize_url = format!(
        "{}/oauth/authorize?response_type=code&client_id={}&code_challenge_method=S256&code_challenge={}&redirect_uri={}&state={}",
        base_url.trim_end_matches('/'),
        urlencoding::encode(HOSTED_OAUTH_CLIENT_ID),
        urlencoding::encode(&challenge),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&state)
    );

    if !no_browser {
        let _ = webbrowser::open(&authorize_url);
    }

    if json {
        eprintln!("Open this URL to authenticate:\n{authorize_url}\n");
    } else {
        println!("Open this URL to authenticate:\n{authorize_url}\n");
    }
    let code = prompt_input("Paste authorization code: ", json)?;
    if code.is_empty() {
        anyhow::bail!("authorization code is required");
    }

    let client = reqwest::Client::new();
    let token_url = format!("{}/oauth/token", base_url.trim_end_matches('/'));
    let body = TokenRequest {
        grant_type: "authorization_code".to_string(),
        client_id: HOSTED_OAUTH_CLIENT_ID.to_string(),
        code,
        code_verifier: verifier,
        redirect_uri: redirect_uri.to_string(),
    };

    let response = client.post(&token_url).json(&body).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "token exchange failed ({}). You can fallback to `claw auth token set --stdin --base-url {} --profile {}`. Body: {}",
            status,
            base_url,
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

    let refresh_token_present = token_response.refresh_token.is_some();
    let mut config = try_load_auth_config()?;
    config.profiles.insert(
        profile.clone(),
        AuthProfile {
            base_url: base_url.clone(),
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at_unix,
        },
    );
    save_auth_config(&config)?;

    if json {
        print_auth_json(serde_json::json!({
            "schema_version": 1,
            "action": "auth.login",
            "profile": profile,
            "base_url": base_url,
            "saved": true,
            "token_present": true,
            "refresh_token_present": refresh_token_present,
            "expires_at_unix": expires_at_unix,
        }))?;
    } else {
        println!("Saved auth profile '{profile}'");
    }
    Ok(())
}

fn logout(profile: String, json: bool) -> anyhow::Result<()> {
    let mut config = try_load_auth_config()?;
    if config.profiles.remove(&profile).is_none() {
        anyhow::bail!("profile '{}' not found", profile);
    }

    save_auth_config(&config)?;
    if json {
        print_auth_json(serde_json::json!({
            "schema_version": 1,
            "action": "auth.logout",
            "profile": profile,
            "removed": true,
        }))?;
    } else {
        println!("Logged out profile '{profile}'");
    }
    Ok(())
}

fn token(command: TokenCommand, json: bool) -> anyhow::Result<()> {
    match command {
        TokenCommand::Set {
            token,
            stdin,
            base_url,
            profile,
        } => {
            let token_source = if stdin { "stdin" } else { "argument" };
            let mut input = io::stdin();
            let token = resolve_token_value(token, stdin, &mut input)?;
            let mut config = try_load_auth_config()?;
            config.profiles.insert(
                profile.clone(),
                AuthProfile {
                    base_url: base_url.clone(),
                    access_token: token,
                    refresh_token: None,
                    expires_at_unix: None,
                },
            );
            save_auth_config(&config)?;
            if json {
                print_auth_json(serde_json::json!({
                    "schema_version": 1,
                    "action": "auth.token.set",
                    "profile": profile,
                    "base_url": base_url,
                    "saved": true,
                    "token_source": token_source,
                    "token_present": true,
                    "refresh_token_present": false,
                    "expires_at_unix": null,
                }))?;
            } else {
                println!("Stored token in profile '{profile}'");
            }
        }
        TokenCommand::Show { profile } => {
            let config = try_load_auth_config()?;
            let entry = config
                .profiles
                .get(&profile)
                .ok_or_else(|| anyhow::anyhow!("profile '{}' not found", profile))?;

            if json {
                print_auth_json(serde_json::json!({
                    "schema_version": 1,
                    "action": "auth.token.show",
                    "profile": profile,
                    "base_url": entry.base_url,
                    "token_present": !entry.access_token.is_empty(),
                    "refresh_token_present": entry.refresh_token.is_some(),
                    "expires_at_unix": entry.expires_at_unix,
                }))?;
            } else {
                println!("profile: {profile}");
                println!("base_url: {}", entry.base_url);
                println!(
                    "access_token: {}",
                    if entry.access_token.is_empty() {
                        "missing"
                    } else {
                        "present"
                    }
                );
                if let Some(exp) = entry.expires_at_unix {
                    println!("expires_at_unix: {exp}");
                }
            }
        }
        TokenCommand::List => {
            let config = try_load_auth_config()?;
            if json {
                let profiles: Vec<_> = config
                    .profiles
                    .iter()
                    .map(|(name, profile)| {
                        serde_json::json!({
                            "profile": name,
                            "base_url": profile.base_url,
                            "token_present": !profile.access_token.is_empty(),
                            "refresh_token_present": profile.refresh_token.is_some(),
                            "expires_at_unix": profile.expires_at_unix,
                        })
                    })
                    .collect();
                print_auth_json(serde_json::json!({
                    "schema_version": 1,
                    "action": "auth.token.list",
                    "profile_count": profiles.len(),
                    "profiles": profiles,
                }))?;
            } else if config.profiles.is_empty() {
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

fn resolve_token_value<R: Read>(
    token: Option<String>,
    stdin: bool,
    reader: &mut R,
) -> anyhow::Result<String> {
    match (token, stdin) {
        (Some(_), true) => anyhow::bail!("pass token as an argument or --stdin, not both"),
        (Some(token), false) => ensure_token_not_empty(token),
        (None, true) => {
            let mut token = String::new();
            reader.read_to_string(&mut token)?;
            let token = token.trim_end_matches(['\r', '\n']).to_string();
            ensure_token_not_empty(token)
        }
        (None, false) => anyhow::bail!("token value is required; pass <token> or --stdin"),
    }
}

fn ensure_token_not_empty(token: String) -> anyhow::Result<String> {
    if token.is_empty() {
        anyhow::bail!("token value is required; pass <token> or --stdin");
    }
    Ok(token)
}

fn print_auth_json(value: serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Cursor;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: AuthArgs,
    }

    #[test]
    fn hosted_login_requires_explicit_base_url() {
        match TestCli::try_parse_from(["claw", "login", "--profile", "prod"]) {
            Ok(_) => panic!("login without --base-url should fail"),
            Err(err) => assert!(err.to_string().contains("--base-url")),
        }
    }

    #[test]
    fn token_set_records_explicit_base_url() {
        let cli = TestCli::parse_from([
            "claw",
            "--json",
            "token",
            "set",
            "token-value",
            "--base-url",
            "https://daemon.example.invalid",
            "--profile",
            "prod",
        ]);
        assert!(cli.args.json);
        match cli.args.command {
            AuthCommand::Token {
                command:
                    TokenCommand::Set {
                        token,
                        stdin,
                        base_url,
                        profile,
                    },
            } => {
                assert_eq!(token.as_deref(), Some("token-value"));
                assert!(!stdin);
                assert_eq!(base_url, "https://daemon.example.invalid");
                assert_eq!(profile, "prod");
            }
            _ => panic!("expected token set"),
        }
    }

    #[test]
    fn token_set_can_read_secret_from_stdin() {
        let cli = TestCli::parse_from([
            "claw",
            "token",
            "set",
            "--stdin",
            "--base-url",
            "https://daemon.example.invalid",
            "--profile",
            "prod",
        ]);
        match cli.args.command {
            AuthCommand::Token {
                command:
                    TokenCommand::Set {
                        token,
                        stdin,
                        base_url,
                        profile,
                    },
            } => {
                assert!(token.is_none());
                assert!(stdin);
                assert_eq!(base_url, "https://daemon.example.invalid");
                assert_eq!(profile, "prod");
            }
            _ => panic!("expected token set"),
        }

        let mut input = Cursor::new("secret-from-stdin\n");
        let token = resolve_token_value(None, true, &mut input).expect("stdin token");
        assert_eq!(token, "secret-from-stdin");
    }

    #[test]
    fn token_set_rejects_ambiguous_or_missing_secret_input() {
        let mut empty = Cursor::new("");
        let missing = resolve_token_value(None, false, &mut empty)
            .expect_err("missing token source should fail");
        assert!(missing.to_string().contains("pass <token> or --stdin"));

        let mut ignored = Cursor::new("stdin-token");
        let ambiguous = resolve_token_value(Some("arg-token".to_string()), true, &mut ignored)
            .expect_err("ambiguous token sources should fail");
        assert!(ambiguous.to_string().contains("as an argument or --stdin"));
    }
}
