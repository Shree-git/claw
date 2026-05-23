//! Command-line interface for Claw VCS.
//!
//! The binary wires Clap parsing, runtime safety profiles, diagnostics, and
//! command dispatch for the `claw` executable.

use std::ffi::OsStr;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::error::ErrorKind;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod auth_store;
mod commands;
mod config;
mod conflict_writer;
mod diff_render;
mod error;
mod ignore;
mod merge_state;
mod output;
mod worktree;

use commands::Commands;
use commands::{ErrorFormat, RuntimeOptions};
use error::CliDiagnostic;

static CLI_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(clap::ValueEnum, Clone, Debug)]
enum ProfileArg {
    Dev,
    Prod,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ErrorFormatArg {
    Human,
    Json,
}

#[derive(Parser)]
#[command(
    name = "claw",
    version,
    about = "Intent-native, agent-native version control"
)]
struct Cli {
    /// Operational profile for safety defaults
    #[arg(long, value_enum, default_value_t = ProfileArg::Prod)]
    profile: ProfileArg,
    /// Validate client/server compatibility before remote operations.
    ///
    /// This is the default; the flag remains for scripts that already pass it.
    #[arg(long)]
    compat_check: bool,
    /// Skip client/server compatibility validation before remote operations
    #[arg(long)]
    no_compat_check: bool,
    /// Output command runtime errors in human or JSON envelope form
    #[arg(long, value_enum, default_value_t = ErrorFormatArg::Human)]
    error_format: ErrorFormatArg,
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let requested_error_format = requested_error_format_from_args(std::env::args_os().skip(1));
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            match (requested_error_format, err.kind()) {
                (ErrorFormat::Json, ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
                    err.print()?;
                }
                (ErrorFormat::Json, kind) => {
                    let diagnostic = CliDiagnostic::from_usage_error(err.to_string(), kind);
                    let request_id = new_cli_request_id()?;
                    print_json_diagnostic(&diagnostic, &request_id)?;
                }
                (ErrorFormat::Human, _) => {
                    err.print()?;
                }
            }
            std::process::exit(err.exit_code());
        }
    };
    let runtime = RuntimeOptions {
        profile: match cli.profile {
            ProfileArg::Dev => "dev".to_string(),
            ProfileArg::Prod => "prod".to_string(),
        },
        compat_check: cli.compat_check || !cli.no_compat_check,
        error_format: match cli.error_format {
            ErrorFormatArg::Human => ErrorFormat::Human,
            ErrorFormatArg::Json => ErrorFormat::Json,
        },
    };

    if let Err(err) = cli.command.run(&runtime).await {
        let diagnostic = CliDiagnostic::from_error(&err);
        let request_id = new_cli_request_id()?;
        match runtime.error_format {
            ErrorFormat::Human => {
                diagnostic.print_human(&request_id);
                std::process::exit(diagnostic.exit_code);
            }
            ErrorFormat::Json => {
                print_json_diagnostic(&diagnostic, &request_id)?;
                std::process::exit(diagnostic.exit_code);
            }
        }
    }

    Ok(())
}

fn requested_error_format_from_args<I, S>(args: I) -> ErrorFormat
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        if arg == "--error-format" {
            return match args.next().and_then(|value| {
                value
                    .as_ref()
                    .to_str()
                    .map(|value| value.eq_ignore_ascii_case("json"))
            }) {
                Some(true) => ErrorFormat::Json,
                _ => ErrorFormat::Human,
            };
        }

        if let Some(value) = arg
            .to_str()
            .and_then(|arg| arg.strip_prefix("--error-format="))
        {
            return if value.eq_ignore_ascii_case("json") {
                ErrorFormat::Json
            } else {
                ErrorFormat::Human
            };
        }
    }

    ErrorFormat::Human
}

fn new_cli_request_id() -> anyhow::Result<String> {
    let counter = CLI_REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "req_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
        counter
    ))
}

fn print_json_diagnostic(diagnostic: &CliDiagnostic, request_id: &str) -> anyhow::Result<()> {
    let envelope = serde_json::json!({
        "schema_version": diagnostic.schema_version,
        "code": diagnostic.code,
        "message": diagnostic.message,
        "reason": diagnostic.reason,
        "request_id": request_id,
        "remediation": diagnostic.remediation,
        "exit_code": diagnostic.exit_code,
        "details": diagnostic.details,
    });
    eprintln!("{}", serde_json::to_string(&envelope)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::commands::ErrorFormat;

    use super::{
        new_cli_request_id, requested_error_format_from_args, Cli, ErrorFormatArg, ProfileArg,
    };

    #[test]
    fn parses_global_flags_defaults() {
        let cli = Cli::parse_from(["claw", "status"]);

        assert!(matches!(cli.profile, ProfileArg::Prod));
        assert!(cli.compat_check || !cli.no_compat_check);
        assert!(matches!(cli.error_format, ErrorFormatArg::Human));
    }

    #[test]
    fn parses_global_flags_explicit_values() {
        let cli = Cli::parse_from([
            "claw",
            "--profile",
            "dev",
            "--compat-check",
            "--error-format",
            "json",
            "status",
        ]);

        assert!(matches!(cli.profile, ProfileArg::Dev));
        assert!(cli.compat_check);
        assert!(!cli.no_compat_check);
        assert!(matches!(cli.error_format, ErrorFormatArg::Json));
    }

    #[test]
    fn parses_global_no_compat_check_escape_hatch() {
        let cli = Cli::parse_from(["claw", "--no-compat-check", "status"]);

        assert!(!cli.compat_check);
        assert!(cli.no_compat_check);
    }

    #[test]
    fn detects_json_error_format_before_parse_success() {
        assert!(matches!(
            requested_error_format_from_args(["--error-format", "json", "wat"]),
            ErrorFormat::Json
        ));
        assert!(matches!(
            requested_error_format_from_args(["--error-format=json", "wat"]),
            ErrorFormat::Json
        ));
        assert!(matches!(
            requested_error_format_from_args(["--error-format", "human", "wat"]),
            ErrorFormat::Human
        ));
    }

    #[test]
    fn cli_request_ids_are_prefixed_and_unique_within_process() {
        let first = new_cli_request_id().expect("first request id");
        let second = new_cli_request_id().expect("second request id");

        assert_cli_request_id_shape(&first);
        assert_cli_request_id_shape(&second);
        assert_ne!(first, second);
    }

    fn assert_cli_request_id_shape(request_id: &str) {
        let parts: Vec<&str> = request_id.split('_').collect();
        assert_eq!(parts.len(), 3, "request id must be req_<millis>_<counter>");
        assert_eq!(parts[0], "req");
        assert!(
            parts[1].parse::<u128>().is_ok(),
            "request id timestamp must be numeric: {request_id}"
        );
        assert!(
            parts[2].parse::<u64>().is_ok(),
            "request id counter must be numeric: {request_id}"
        );
    }
}
