use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Args)]
pub struct PluginArgs {
    /// Emit machine-readable JSON output
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Check plugin JSON-RPC initialize handshake
    Check(CheckArgs),
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Protocol version to negotiate
    #[arg(long, default_value_t = 1)]
    protocol: u32,
    /// Path to plugin executable
    #[arg(long)]
    plugin: PathBuf,
    /// Timeout for initialize round-trip
    #[arg(long, default_value_t = 30_000)]
    timeout_ms: u64,
}

#[derive(Debug)]
enum InitializeResponse {
    Success,
    Error {
        code: Option<String>,
        message: String,
    },
}

pub async fn run(args: PluginArgs) -> anyhow::Result<()> {
    match args.command {
        PluginCommand::Check(check_args) => run_check(check_args, args.json).await,
    }
}

async fn run_check(args: CheckArgs, json_output: bool) -> anyhow::Result<()> {
    let request_id = "init-1";
    let request_line = json!({
        "jsonrpc": "1.0",
        "id": request_id,
        "method": "plugin.initialize",
        "params": {
            "protocolVersion": args.protocol.to_string(),
            "host": {
                "name": "claw",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "streaming": false,
                "tools": [],
                "maxRequestTimeoutMs": args.timeout_ms,
            }
        }
    })
    .to_string();

    let mut child = Command::new(&args.plugin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "failed to start plugin executable at {}",
                args.plugin.display()
            )
        })?;

    let mut stdin = child
        .stdin
        .take()
        .context("failed to open plugin stdin pipe")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to open plugin stdout pipe")?;

    stdin.write_all(request_line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    drop(stdin);

    let mut reader = BufReader::new(stdout);
    let mut response_line = String::new();
    let timeout_duration = Duration::from_millis(args.timeout_ms);
    let read_len = timeout(timeout_duration, reader.read_line(&mut response_line))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "plugin did not respond within {}ms. Increase --timeout-ms or inspect plugin startup.",
                args.timeout_ms
            )
        })??;

    terminate_child(&mut child).await;

    if read_len == 0 {
        anyhow::bail!(
            "plugin exited without an initialize response. Ensure it writes JSON-RPC to stdout."
        );
    }

    match validate_initialize_response(response_line.trim(), request_id)? {
        InitializeResponse::Success => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&plugin_check_receipt(&args, request_id, true))?
                );
                return Ok(());
            }
            println!(
                "Plugin check passed: {} (protocol {})",
                display_plugin_path(&args.plugin),
                args.protocol
            );
            Ok(())
        }
        InitializeResponse::Error { code, message } => {
            if code.as_deref() == Some("UNSUPPORTED_VERSION") {
                anyhow::bail!(
                    "plugin rejected protocol {} (UNSUPPORTED_VERSION): {}. Retry with --protocol <supported-version>.",
                    args.protocol,
                    message
                );
            }
            anyhow::bail!(
                "plugin initialize failed{}: {}",
                format_error_code(code),
                message
            );
        }
    }
}

fn plugin_check_receipt(args: &CheckArgs, request_id: &str, ok: bool) -> Value {
    json!({
        "schema_version": 1,
        "action": "plugin.check",
        "ok": ok,
        "plugin": display_plugin_path(&args.plugin),
        "protocol": args.protocol,
        "timeout_ms": args.timeout_ms,
        "request_id": request_id,
        "jsonrpc": "1.0",
        "method": "plugin.initialize",
    })
}

fn display_plugin_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn format_error_code(code: Option<String>) -> String {
    match code {
        Some(code) => format!(" [{}]", code),
        None => String::new(),
    }
}

fn validate_initialize_response(
    line: &str,
    request_id: &str,
) -> anyhow::Result<InitializeResponse> {
    let value: Value = serde_json::from_str(line).context("response is not valid JSON")?;
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("response must be a JSON object"))?;

    let jsonrpc = obj
        .get("jsonrpc")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("response missing string field 'jsonrpc'"))?;
    if jsonrpc != "1.0" {
        anyhow::bail!("response jsonrpc must be '1.0', got '{jsonrpc}'");
    }

    let id = obj
        .get("id")
        .ok_or_else(|| anyhow::anyhow!("response missing field 'id'"))?;
    if id != &Value::String(request_id.to_string()) {
        anyhow::bail!("response id mismatch: expected '{request_id}', got {id}");
    }

    let result = obj.get("result");
    let error = obj.get("error");
    let has_result = matches!(result, Some(v) if !v.is_null());
    let has_error = matches!(error, Some(v) if !v.is_null());

    if has_result == has_error {
        anyhow::bail!(
            "response must include exactly one non-null field among 'result' and 'error'"
        );
    }

    if has_result {
        return Ok(InitializeResponse::Success);
    }

    let Some(error_value) = error else {
        anyhow::bail!("response must include an error object when 'error' is non-null");
    };
    let (code, message) = extract_error_fields(error_value);
    Ok(InitializeResponse::Error { code, message })
}

fn extract_error_fields(error_value: &Value) -> (Option<String>, String) {
    if let Some(obj) = error_value.as_object() {
        let code = obj
            .get("code")
            .map(value_to_compact_string)
            .filter(|code| !code.is_empty());
        let message = obj
            .get("message")
            .map(value_to_compact_string)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "plugin returned an error without message".to_string());
        return (code, message);
    }
    (None, value_to_compact_string(error_value))
}

fn value_to_compact_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.to_string(),
        _ => value.to_string(),
    }
}

async fn terminate_child(child: &mut tokio::process::Child) {
    match timeout(Duration::from_millis(100), child.wait()).await {
        Ok(_) => {}
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Debug, Subcommand)]
    enum TestCommand {
        Plugin(PluginArgs),
    }

    #[test]
    fn parses_plugin_check_defaults() {
        let cli = TestCli::parse_from(["claw", "plugin", "check", "--plugin", "/tmp/plugin"]);

        let TestCommand::Plugin(plugin_args) = cli.command;
        assert!(!plugin_args.json);
        match plugin_args.command {
            PluginCommand::Check(args) => {
                assert_eq!(args.protocol, 1);
                assert_eq!(args.timeout_ms, 30_000);
                assert_eq!(args.plugin, PathBuf::from("/tmp/plugin"));
            }
        }
    }

    #[test]
    fn parses_plugin_check_explicit_values() {
        let cli = TestCli::parse_from([
            "claw",
            "plugin",
            "--json",
            "check",
            "--plugin",
            "./bin/example-plugin",
            "--protocol",
            "2",
            "--timeout-ms",
            "45000",
        ]);

        let TestCommand::Plugin(plugin_args) = cli.command;
        assert!(plugin_args.json);
        match plugin_args.command {
            PluginCommand::Check(args) => {
                assert_eq!(args.protocol, 2);
                assert_eq!(args.timeout_ms, 45_000);
                assert_eq!(args.plugin, PathBuf::from("./bin/example-plugin"));
            }
        }
    }

    #[test]
    fn parser_requires_plugin_argument() {
        let err = TestCli::try_parse_from(["claw", "plugin", "check"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn plugin_check_json_receipt_is_v1_and_non_secret() {
        let args = CheckArgs {
            protocol: 1,
            plugin: PathBuf::from("./bin/example-plugin"),
            timeout_ms: 30_000,
        };

        let receipt = plugin_check_receipt(&args, "init-1", true);

        assert_eq!(receipt["schema_version"], 1);
        assert_eq!(receipt["action"], "plugin.check");
        assert_eq!(receipt["ok"], true);
        assert_eq!(receipt["plugin"], "./bin/example-plugin");
        assert_eq!(receipt["protocol"], 1);
        assert_eq!(receipt["timeout_ms"], 30_000);
        assert_eq!(receipt["request_id"], "init-1");
        assert_eq!(receipt["jsonrpc"], "1.0");
        assert_eq!(receipt["method"], "plugin.initialize");
        assert!(
            receipt.get("stdin").is_none() && receipt.get("stderr").is_none(),
            "plugin check receipt must not preserve plugin streams"
        );
    }

    #[test]
    fn validates_success_response() {
        let line = r#"{"jsonrpc":"1.0","id":"init-1","result":{"ok":true},"error":null}"#;
        let response = validate_initialize_response(line, "init-1").unwrap();
        assert!(matches!(response, InitializeResponse::Success));
    }

    #[test]
    fn rejects_response_with_mismatched_id() {
        let line = r#"{"jsonrpc":"1.0","id":"other","result":{"ok":true},"error":null}"#;
        let err = validate_initialize_response(line, "init-1").unwrap_err();
        assert!(err.to_string().contains("id mismatch"));
    }

    #[test]
    fn rejects_response_with_both_result_and_error() {
        let line = r#"{"jsonrpc":"1.0","id":"init-1","result":{"ok":true},"error":{"code":"INTERNAL_ERROR","message":"boom"}}"#;
        let err = validate_initialize_response(line, "init-1").unwrap_err();
        assert!(err
            .to_string()
            .contains("exactly one non-null field among 'result' and 'error'"));
    }

    #[test]
    fn parses_error_response_details() {
        let line = r#"{"jsonrpc":"1.0","id":"init-1","result":null,"error":{"code":"UNSUPPORTED_VERSION","message":"v2 only"}}"#;
        let response = validate_initialize_response(line, "init-1").unwrap();
        match response {
            InitializeResponse::Error { code, message } => {
                assert_eq!(code.as_deref(), Some("UNSUPPORTED_VERSION"));
                assert_eq!(message, "v2 only");
            }
            InitializeResponse::Success => panic!("expected error response"),
        }
    }
}
