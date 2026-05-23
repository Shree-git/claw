use std::ffi::OsString;
use std::process::Stdio;

use anyhow::Context;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Args)]
pub struct McpArgs {
    #[command(subcommand)]
    command: McpCommand,
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    /// Run the Claw MCP server over newline-delimited JSON-RPC on stdio
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
struct ServeArgs {
    /// Claw binary to execute for tool calls
    #[arg(long, default_value = "claw")]
    claw_binary: String,
    /// Allow repository-mutating MCP tools
    #[arg(long)]
    allow_write: bool,
}

#[derive(Debug, Clone)]
struct ToolSpec {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
    write: bool,
}

pub async fn run(args: McpArgs) -> anyhow::Result<()> {
    match args.command {
        McpCommand::Serve(args) => run_serve(args).await,
    }
}

async fn run_serve(args: ServeArgs) -> anyhow::Result<()> {
    let mut reader = BufReader::new(io::stdin());
    let mut stdout = io::stdout();
    let mut line = String::new();
    let server = McpServer {
        claw_binary: args.claw_binary,
        allow_write: args.allow_write,
    };

    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(trimmed).await {
            stdout.write_all(response.to_string().as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

struct McpServer {
    claw_binary: String,
    allow_write: bool,
}

impl McpServer {
    async fn handle_line(&self, line: &str) -> Option<Value> {
        let request = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(err) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    "Parse error",
                    json!({ "error": err.to_string() }),
                ));
            }
        };
        let id = request.get("id").cloned();
        let Some(id) = id else {
            return None;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => Ok(self.initialize_result()),
            "tools/list" => Ok(json!({ "tools": tool_specs(self.allow_write) })),
            "tools/call" => self.handle_tool_call(request.get("params")).await,
            "ping" => Ok(json!({})),
            _ => Err((-32601, format!("unknown MCP method: {method}"), Value::Null)),
        };

        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message, data)) => error_response(id, code, &message, data),
        })
    }

    fn initialize_result(&self) -> Value {
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "claw",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Use Claw MCP tools to inspect intent, change, evidence, policy, and trust state in the current repository."
        })
    }

    async fn handle_tool_call(
        &self,
        params: Option<&Value>,
    ) -> Result<Value, (i32, String, Value)> {
        let Some(params) = params else {
            return Err((
                -32602,
                "tools/call requires params".to_string(),
                Value::Null,
            ));
        };
        let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
            (
                -32602,
                "tools/call requires string params.name".to_string(),
                params.clone(),
            )
        })?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let (write, args) = tool_command(name, &arguments)?;
        if write && !self.allow_write {
            return Err((
                -32000,
                format!("tool '{name}' requires --allow-write"),
                json!({ "tool": name }),
            ));
        }
        let value = run_claw_json(&self.claw_binary, args)
            .await
            .map_err(|err| {
                (
                    -32000,
                    format!("claw tool '{name}' failed"),
                    json!({ "tool": name, "error": err.to_string() }),
                )
            })?;
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            }],
            "structuredContent": value,
            "isError": false
        }))
    }
}

fn tool_specs(allow_write: bool) -> Vec<Value> {
    tools()
        .into_iter()
        .filter(|tool| allow_write || !tool.write)
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })
        })
        .collect()
}

fn tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "claw_status",
            description: "Return repository branch, merge state, and working tree changes.",
            input_schema: object_schema(vec![]),
            write: false,
        },
        ToolSpec {
            name: "claw_intent_list",
            description: "List Claw intents.",
            input_schema: object_schema(vec![]),
            write: false,
        },
        ToolSpec {
            name: "claw_intent_graph",
            description:
                "Return the intent graph with goals, changes, revisions, capsules, evidence, policies, agents, and blockers.",
            input_schema: object_schema(vec![]),
            write: false,
        },
        ToolSpec {
            name: "claw_intent_create",
            description:
                "Create an intent with a title, goal, and optional executable acceptance tests.",
            input_schema: object_schema(vec![
                ("title", json!({ "type": "string" })),
                ("goal", json!({ "type": "string" })),
                (
                    "acceptance_tests",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
            ]),
            write: true,
        },
        ToolSpec {
            name: "claw_run_acceptance",
            description: "Run executable acceptance tests linked to an intent.",
            input_schema: object_schema(vec![
                ("intent_id", json!({ "type": "string" })),
                ("timeout_ms", json!({ "type": "integer", "minimum": 1 })),
                ("keep_going", json!({ "type": "boolean" })),
            ]),
            write: true,
        },
        ToolSpec {
            name: "claw_change_create",
            description: "Create a change linked to an existing intent.",
            input_schema: object_schema(vec![("intent_id", json!({ "type": "string" }))]),
            write: true,
        },
        ToolSpec {
            name: "claw_evidence_query",
            description: "Query capsule evidence, for example test=pass AND signer.trust>0.8.",
            input_schema: object_schema(vec![
                ("query", json!({ "type": "string" })),
                ("revision", json!({ "type": "string" })),
                ("capsule", json!({ "type": "string" })),
                ("limit", json!({ "type": "integer", "minimum": 1 })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_doctor",
            description:
                "Return repository health checks, optionally including the deep repairability scan.",
            input_schema: object_schema(vec![
                ("deep", json!({ "type": "boolean" })),
                ("strict", json!({ "type": "boolean" })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_repair_plan",
            description: "Return the non-mutating repair plan for repository health issues.",
            input_schema: object_schema(vec![]),
            write: false,
        },
        ToolSpec {
            name: "claw_story_export",
            description: "Export an intent/change history as a structured audit narrative.",
            input_schema: object_schema(vec![("intent", json!({ "type": "string" }))]),
            write: false,
        },
        ToolSpec {
            name: "claw_timeline_allowed",
            description:
                "Explain when a revision became allowed and which current policies allowed it.",
            input_schema: object_schema(vec![
                ("revision", json!({ "type": "string" })),
                (
                    "policies",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "signer_agents",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "signer_keys",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "paths",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_patch_workbench",
            description: "Explain whether two patch objects commute, conflict, invert, or reorder.",
            input_schema: object_schema(vec![
                ("left", json!({ "type": "string" })),
                ("right", json!({ "type": "string" })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_capsule_inspect",
            description:
                "Inspect capsule identity, evidence, signatures, private-field metadata, and trust path.",
            input_schema: object_schema(vec![("target", json!({ "type": "string" }))]),
            write: false,
        },
        ToolSpec {
            name: "claw_review",
            description: "Return review data organized by intent, change, revision, and capsule.",
            input_schema: object_schema(vec![
                ("intent", json!({ "type": "string" })),
                ("change", json!({ "type": "string" })),
                ("capsule", json!({ "type": "string" })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_agent_audit",
            description: "Audit registered agent keys and lifecycle state.",
            input_schema: object_schema(vec![
                ("status", json!({ "type": "string" })),
                ("risk", json!({ "type": "string" })),
                ("action_required", json!({ "type": "boolean" })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_agent_bulk",
            description:
                "Apply or dry-run an ordered JSON plan of agent register, rotate, revoke, quarantine, and unquarantine operations.",
            input_schema: object_schema(vec![
                ("file", json!({ "type": "string" })),
                ("dry_run", json!({ "type": "boolean" })),
            ]),
            write: true,
        },
        ToolSpec {
            name: "claw_trust_receipt",
            description: "Explain why a revision is trustworthy under capsule and policy evidence.",
            input_schema: object_schema(vec![
                ("revision", json!({ "type": "string" })),
                ("capsule", json!({ "type": "string" })),
                (
                    "policies",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "signer_agents",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "signer_keys",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "paths",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                ("trust_score", json!({ "type": "string" })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_policy_simulate",
            description:
                "Dry-run a stored or file-backed policy against a revision and capsule and explain pass/fail reasons.",
            input_schema: object_schema(vec![
                ("policy_id", json!({ "type": "string" })),
                ("policy_file", json!({ "type": "string" })),
                ("revision", json!({ "type": "string" })),
                ("capsule", json!({ "type": "string" })),
                (
                    "signer_agents",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "signer_keys",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                (
                    "paths",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                ("trust_score", json!({ "type": "string" })),
            ]),
            write: false,
        },
        ToolSpec {
            name: "claw_provenance_replay",
            description:
                "Replay claimed command evidence and optionally attach replay comparison evidence.",
            input_schema: object_schema(vec![
                ("revision", json!({ "type": "string" })),
                ("capsule", json!({ "type": "string" })),
                (
                    "evidence_names",
                    json!({ "type": "array", "items": { "type": "string" } }),
                ),
                ("agent", json!({ "type": "string" })),
                ("timeout_ms", json!({ "type": "integer", "minimum": 1 })),
                ("keep_going", json!({ "type": "boolean" })),
                ("in_place", json!({ "type": "boolean" })),
                ("dry_run", json!({ "type": "boolean" })),
            ]),
            write: true,
        },
        ToolSpec {
            name: "claw_attach_attestation",
            description: "Attach a SLSA or in-toto attestation as signed capsule evidence.",
            input_schema: object_schema(vec![
                ("revision", json!({ "type": "string" })),
                ("capsule", json!({ "type": "string" })),
                ("file", json!({ "type": "string" })),
                ("agent", json!({ "type": "string" })),
                ("subject_name", json!({ "type": "string" })),
                ("subject_digest", json!({ "type": "string" })),
                ("builder_id", json!({ "type": "string" })),
                ("build_type", json!({ "type": "string" })),
                ("dry_run", json!({ "type": "boolean" })),
            ]),
            write: true,
        },
    ]
}

fn object_schema(properties: Vec<(&'static str, Value)>) -> Value {
    let required = properties
        .iter()
        .filter_map(|(name, value)| {
            if matches!(
                *name,
                "title"
                    | "intent"
                    | "intent_id"
                    | "query"
                    | "revision"
                    | "target"
                    | "file"
                    | "left"
                    | "right"
            ) {
                Some(Value::String((*name).to_string()))
            } else if *name == "goal" && value.get("type").and_then(Value::as_str) == Some("string")
            {
                Some(Value::String((*name).to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let properties = properties
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn tool_command(
    name: &str,
    arguments: &Value,
) -> Result<(bool, Vec<OsString>), (i32, String, Value)> {
    match name {
        "claw_status" => Ok((false, args(&["status", "--json"]))),
        "claw_intent_list" => Ok((false, args(&["intent", "--json", "list"]))),
        "claw_intent_graph" => Ok((false, args(&["intent", "--json", "graph"]))),
        "claw_intent_create" => {
            let title = required_string(arguments, "title")?;
            let goal = required_string(arguments, "goal")?;
            let mut out = args(&[
                "intent", "--json", "create", "--title", &title, "--goal", &goal,
            ]);
            for test in optional_string_array(arguments, "acceptance_tests")? {
                out.push("--acceptance-test".into());
                out.push(test.into());
            }
            Ok((true, out))
        }
        "claw_run_acceptance" => {
            let intent_id = required_string(arguments, "intent_id")?;
            let mut out = args(&["intent", "--json", "run-acceptance", &intent_id]);
            push_optional_integer(&mut out, arguments, "timeout_ms", "--timeout-ms")?;
            push_optional_bool_flag(&mut out, arguments, "keep_going", "--keep-going")?;
            Ok((true, out))
        }
        "claw_change_create" => {
            let intent_id = required_string(arguments, "intent_id")?;
            Ok((
                true,
                args(&["change", "--json", "create", "--intent", &intent_id]),
            ))
        }
        "claw_evidence_query" => {
            let query = required_string(arguments, "query")?;
            let mut out = args(&["evidence", "--json", "query", &query]);
            push_optional_string(&mut out, arguments, "revision", "--revision")?;
            push_optional_string(&mut out, arguments, "capsule", "--capsule")?;
            push_optional_integer(&mut out, arguments, "limit", "--limit")?;
            Ok((false, out))
        }
        "claw_doctor" => {
            let mut out = args(&["doctor", "--json"]);
            push_optional_bool_flag(&mut out, arguments, "deep", "--deep")?;
            push_optional_bool_flag(&mut out, arguments, "strict", "--strict")?;
            Ok((false, out))
        }
        "claw_repair_plan" => Ok((false, args(&["repair", "--json", "plan"]))),
        "claw_story_export" => {
            let intent = required_string(arguments, "intent")?;
            Ok((
                false,
                args(&["story", "export", "--intent", &intent, "--format", "json"]),
            ))
        }
        "claw_timeline_allowed" => {
            let revision = required_string(arguments, "revision")?;
            let mut out = args(&["timeline", "--json", "allowed", "--revision", &revision]);
            for policy in optional_string_array(arguments, "policies")? {
                out.push("--policy".into());
                out.push(policy.into());
            }
            for agent in optional_string_array(arguments, "signer_agents")? {
                out.push("--signer-agent".into());
                out.push(agent.into());
            }
            for key in optional_string_array(arguments, "signer_keys")? {
                out.push("--signer-key".into());
                out.push(key.into());
            }
            for path in optional_string_array(arguments, "paths")? {
                out.push("--path".into());
                out.push(path.into());
            }
            Ok((false, out))
        }
        "claw_patch_workbench" => {
            let left = required_string(arguments, "left")?;
            let right = required_string(arguments, "right")?;
            Ok((
                false,
                args(&[
                    "patch",
                    "--json",
                    "workbench",
                    "--left",
                    &left,
                    "--right",
                    &right,
                ]),
            ))
        }
        "claw_capsule_inspect" => {
            let target = required_string(arguments, "target")?;
            Ok((false, args(&["capsule", "--json", "inspect", &target])))
        }
        "claw_review" => {
            let mut out = args(&["review", "--json"]);
            push_optional_string(&mut out, arguments, "intent", "--intent")?;
            push_optional_string(&mut out, arguments, "change", "--change")?;
            push_optional_string(&mut out, arguments, "capsule", "--capsule")?;
            Ok((false, out))
        }
        "claw_agent_audit" => {
            let mut out = args(&["agent", "--json", "audit"]);
            push_optional_string(&mut out, arguments, "status", "--status")?;
            push_optional_string(&mut out, arguments, "risk", "--risk")?;
            push_optional_bool_flag(&mut out, arguments, "action_required", "--action-required")?;
            Ok((false, out))
        }
        "claw_agent_bulk" => {
            let file = required_string(arguments, "file")?;
            let mut out = args(&["agent", "--json", "bulk", "--file", &file]);
            push_optional_bool_flag(&mut out, arguments, "dry_run", "--dry-run")?;
            Ok((true, out))
        }
        "claw_trust_receipt" => {
            let revision = required_string(arguments, "revision")?;
            let mut out = args(&["trust", "--json", "receipt", "--revision", &revision]);
            push_optional_string(&mut out, arguments, "capsule", "--capsule")?;
            for policy in optional_string_array(arguments, "policies")? {
                out.push("--policy".into());
                out.push(policy.into());
            }
            for agent in optional_string_array(arguments, "signer_agents")? {
                out.push("--signer-agent".into());
                out.push(agent.into());
            }
            for key in optional_string_array(arguments, "signer_keys")? {
                out.push("--signer-key".into());
                out.push(key.into());
            }
            for path in optional_string_array(arguments, "paths")? {
                out.push("--path".into());
                out.push(path.into());
            }
            push_optional_string(&mut out, arguments, "trust_score", "--trust-score")?;
            Ok((false, out))
        }
        "claw_policy_simulate" => {
            let revision = required_string(arguments, "revision")?;
            let mut out = args(&["policy", "simulate"]);
            if let Some(policy_file) = optional_string(arguments, "policy_file")? {
                out.push("--policy-file".into());
                out.push(policy_file.into());
            } else {
                out.push(required_string(arguments, "policy_id")?.into());
            }
            out.push("--revision".into());
            out.push(revision.into());
            push_optional_string(&mut out, arguments, "capsule", "--capsule")?;
            for agent in optional_string_array(arguments, "signer_agents")? {
                out.push("--signer-agent".into());
                out.push(agent.into());
            }
            for key in optional_string_array(arguments, "signer_keys")? {
                out.push("--signer-key".into());
                out.push(key.into());
            }
            for path in optional_string_array(arguments, "paths")? {
                out.push("--path".into());
                out.push(path.into());
            }
            push_optional_string(&mut out, arguments, "trust_score", "--trust-score")?;
            out.push("--json".into());
            Ok((false, out))
        }
        "claw_provenance_replay" => {
            let revision = required_string(arguments, "revision")?;
            let mut out = args(&["provenance", "--json", "replay", "--revision", &revision]);
            push_optional_string(&mut out, arguments, "capsule", "--capsule")?;
            for evidence in optional_string_array(arguments, "evidence_names")? {
                out.push("--evidence".into());
                out.push(evidence.into());
            }
            push_optional_string(&mut out, arguments, "agent", "--agent")?;
            push_optional_integer(&mut out, arguments, "timeout_ms", "--timeout-ms")?;
            push_optional_bool_flag(&mut out, arguments, "keep_going", "--keep-going")?;
            push_optional_bool_flag(&mut out, arguments, "in_place", "--in-place")?;
            push_optional_bool_flag(&mut out, arguments, "dry_run", "--dry-run")?;
            Ok((true, out))
        }
        "claw_attach_attestation" => {
            let revision = required_string(arguments, "revision")?;
            let file = required_string(arguments, "file")?;
            let mut out = args(&[
                "provenance",
                "--json",
                "attach-attestation",
                "--revision",
                &revision,
                "--file",
                &file,
            ]);
            push_optional_string(&mut out, arguments, "capsule", "--capsule")?;
            push_optional_string(&mut out, arguments, "agent", "--agent")?;
            push_optional_string(&mut out, arguments, "subject_name", "--subject-name")?;
            push_optional_string(&mut out, arguments, "subject_digest", "--subject-digest")?;
            push_optional_string(&mut out, arguments, "builder_id", "--builder-id")?;
            push_optional_string(&mut out, arguments, "build_type", "--build-type")?;
            push_optional_bool_flag(&mut out, arguments, "dry_run", "--dry-run")?;
            Ok((true, out))
        }
        _ => Err((
            -32602,
            format!("unknown MCP tool: {name}"),
            json!({ "tool": name }),
        )),
    }
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn required_string(arguments: &Value, key: &str) -> Result<String, (i32, String, Value)> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            (
                -32602,
                format!("missing non-empty string argument '{key}'"),
                arguments.clone(),
            )
        })
}

fn optional_string_array(
    arguments: &Value,
    key: &str,
) -> Result<Vec<String>, (i32, String, Value)> {
    let Some(value) = arguments.get(key) else {
        return Ok(vec![]);
    };
    let Some(items) = value.as_array() else {
        return Err((
            -32602,
            format!("argument '{key}' must be an array of strings"),
            value.clone(),
        ));
    };
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                (
                    -32602,
                    format!("argument '{key}' must be an array of strings"),
                    value.clone(),
                )
            })
        })
        .collect()
}

fn optional_string(arguments: &Value, key: &str) -> Result<Option<String>, (i32, String, Value)> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let Some(value) = value.as_str() else {
        return Err((
            -32602,
            format!("argument '{key}' must be a string"),
            value.clone(),
        ));
    };
    if value.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

fn push_optional_string(
    out: &mut Vec<OsString>,
    arguments: &Value,
    key: &str,
    flag: &str,
) -> Result<(), (i32, String, Value)> {
    let Some(value) = arguments.get(key) else {
        return Ok(());
    };
    let Some(value) = value.as_str() else {
        return Err((
            -32602,
            format!("argument '{key}' must be a string"),
            value.clone(),
        ));
    };
    out.push(flag.into());
    out.push(value.into());
    Ok(())
}

fn push_optional_integer(
    out: &mut Vec<OsString>,
    arguments: &Value,
    key: &str,
    flag: &str,
) -> Result<(), (i32, String, Value)> {
    let Some(value) = arguments.get(key) else {
        return Ok(());
    };
    let Some(value) = value.as_u64() else {
        return Err((
            -32602,
            format!("argument '{key}' must be a positive integer"),
            value.clone(),
        ));
    };
    out.push(flag.into());
    out.push(value.to_string().into());
    Ok(())
}

fn push_optional_bool_flag(
    out: &mut Vec<OsString>,
    arguments: &Value,
    key: &str,
    flag: &str,
) -> Result<(), (i32, String, Value)> {
    let Some(value) = arguments.get(key) else {
        return Ok(());
    };
    let Some(enabled) = value.as_bool() else {
        return Err((
            -32602,
            format!("argument '{key}' must be a boolean"),
            value.clone(),
        ));
    };
    if enabled {
        out.push(flag.into());
    }
    Ok(())
}

async fn run_claw_json(binary: &str, args: Vec<OsString>) -> anyhow::Result<Value> {
    let output = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("failed to start claw binary '{binary}'"))?;
    if !output.status.success() {
        anyhow::bail!(
            "exit={} stderr={} stdout={}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim(),
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(serde_json::from_str(&stdout)?)
}

fn error_response(id: Value, code: i32, message: &str, data: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": data,
        }
    })
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
        Mcp(McpArgs),
    }

    #[test]
    fn parses_mcp_serve_defaults() {
        let cli = TestCli::parse_from(["claw", "mcp", "serve"]);
        let TestCommand::Mcp(args) = cli.command;
        match args.command {
            McpCommand::Serve(args) => {
                assert_eq!(args.claw_binary, "claw");
                assert!(!args.allow_write);
            }
        }
    }

    #[test]
    fn read_only_tool_list_omits_write_tools() {
        let names = tool_specs(false)
            .into_iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(names.contains(&"claw_status".to_string()));
        assert!(names.contains(&"claw_intent_graph".to_string()));
        assert!(names.contains(&"claw_capsule_inspect".to_string()));
        assert!(names.contains(&"claw_doctor".to_string()));
        assert!(names.contains(&"claw_repair_plan".to_string()));
        assert!(names.contains(&"claw_story_export".to_string()));
        assert!(names.contains(&"claw_timeline_allowed".to_string()));
        assert!(names.contains(&"claw_patch_workbench".to_string()));
        assert!(names.contains(&"claw_policy_simulate".to_string()));
        assert!(!names.contains(&"claw_intent_create".to_string()));
        assert!(!names.contains(&"claw_run_acceptance".to_string()));
        assert!(!names.contains(&"claw_provenance_replay".to_string()));
    }

    #[test]
    fn write_tool_list_includes_mutating_tools() {
        let names = tool_specs(true)
            .into_iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(names.contains(&"claw_intent_create".to_string()));
        assert!(names.contains(&"claw_change_create".to_string()));
        assert!(names.contains(&"claw_run_acceptance".to_string()));
        assert!(names.contains(&"claw_provenance_replay".to_string()));
        assert!(names.contains(&"claw_attach_attestation".to_string()));
    }

    #[test]
    fn builds_evidence_query_command() {
        let (_, args) = tool_command(
            "claw_evidence_query",
            &json!({ "query": "test=pass", "limit": 2 }),
        )
        .unwrap();
        let rendered = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec!["evidence", "--json", "query", "test=pass", "--limit", "2"]
        );
    }

    #[test]
    fn builds_doctor_and_repair_plan_commands() {
        let (_, doctor_args) = tool_command("claw_doctor", &json!({ "deep": true })).unwrap();
        let doctor_rendered = doctor_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(doctor_rendered, vec!["doctor", "--json", "--deep"]);

        let (_, repair_args) = tool_command("claw_repair_plan", &json!({})).unwrap();
        let repair_rendered = repair_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(repair_rendered, vec!["repair", "--json", "plan"]);
    }

    #[test]
    fn builds_story_timeline_and_patch_workbench_commands() {
        let (_, story_args) =
            tool_command("claw_story_export", &json!({ "intent": "01HINTENT" })).unwrap();
        let story_rendered = story_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            story_rendered,
            vec![
                "story",
                "export",
                "--intent",
                "01HINTENT",
                "--format",
                "json"
            ]
        );

        let (_, timeline_args) = tool_command(
            "claw_timeline_allowed",
            &json!({
                "revision": "heads/main",
                "policies": ["release"],
                "signer_agents": ["ci"],
                "paths": ["src/lib.rs"]
            }),
        )
        .unwrap();
        let timeline_rendered = timeline_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            timeline_rendered,
            vec![
                "timeline",
                "--json",
                "allowed",
                "--revision",
                "heads/main",
                "--policy",
                "release",
                "--signer-agent",
                "ci",
                "--path",
                "src/lib.rs",
            ]
        );

        let (_, patch_args) = tool_command(
            "claw_patch_workbench",
            &json!({ "left": "clw_left", "right": "clw_right" }),
        )
        .unwrap();
        let patch_rendered = patch_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            patch_rendered,
            vec![
                "patch",
                "--json",
                "workbench",
                "--left",
                "clw_left",
                "--right",
                "clw_right",
            ]
        );
    }

    #[test]
    fn builds_policy_simulate_command() {
        let (_, args) = tool_command(
            "claw_policy_simulate",
            &json!({
                "policy_id": "ci-required",
                "revision": "heads/main",
                "capsule": "capsules/main",
                "signer_agents": ["ci"],
                "signer_keys": ["key-1"],
                "paths": ["src/lib.rs"],
                "trust_score": "0.9"
            }),
        )
        .unwrap();
        let rendered = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![
                "policy",
                "simulate",
                "ci-required",
                "--revision",
                "heads/main",
                "--capsule",
                "capsules/main",
                "--signer-agent",
                "ci",
                "--signer-key",
                "key-1",
                "--path",
                "src/lib.rs",
                "--trust-score",
                "0.9",
                "--json",
            ]
        );
    }

    #[test]
    fn builds_agent_fleet_commands() {
        let (_, audit_args) = tool_command(
            "claw_agent_audit",
            &json!({
                "status": "quarantined",
                "risk": "warning",
                "action_required": true
            }),
        )
        .unwrap();
        let audit_rendered = audit_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            audit_rendered,
            vec![
                "agent",
                "--json",
                "audit",
                "--status",
                "quarantined",
                "--risk",
                "warning",
                "--action-required",
            ]
        );

        let (bulk_write, bulk_args) = tool_command(
            "claw_agent_bulk",
            &json!({ "file": "agents.json", "dry_run": true }),
        )
        .unwrap();
        let bulk_rendered = bulk_args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(bulk_write);
        assert_eq!(
            bulk_rendered,
            vec![
                "agent",
                "--json",
                "bulk",
                "--file",
                "agents.json",
                "--dry-run",
            ]
        );
    }

    #[test]
    fn builds_trust_receipt_command_with_evaluator_context() {
        let (_, args) = tool_command(
            "claw_trust_receipt",
            &json!({
                "revision": "heads/main",
                "capsule": "capsules/main",
                "policies": ["ci-required"],
                "signer_agents": ["ci"],
                "signer_keys": ["key-1"],
                "paths": ["src/lib.rs"],
                "trust_score": "0.9"
            }),
        )
        .unwrap();
        let rendered = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![
                "trust",
                "--json",
                "receipt",
                "--revision",
                "heads/main",
                "--capsule",
                "capsules/main",
                "--policy",
                "ci-required",
                "--signer-agent",
                "ci",
                "--signer-key",
                "key-1",
                "--path",
                "src/lib.rs",
                "--trust-score",
                "0.9",
            ]
        );
    }

    #[test]
    fn builds_provenance_replay_command() {
        let (_, args) = tool_command(
            "claw_provenance_replay",
            &json!({
                "revision": "heads/main",
                "evidence_names": ["test"],
                "timeout_ms": 1000,
                "keep_going": true,
                "dry_run": true
            }),
        )
        .unwrap();
        let rendered = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![
                "provenance",
                "--json",
                "replay",
                "--revision",
                "heads/main",
                "--evidence",
                "test",
                "--timeout-ms",
                "1000",
                "--keep-going",
                "--dry-run",
            ]
        );
    }

    #[test]
    fn builds_attach_attestation_command() {
        let (_, args) = tool_command(
            "claw_attach_attestation",
            &json!({
                "revision": "heads/main",
                "file": "attestation.json",
                "subject_digest": "sha256:abc",
                "builder_id": "builder-a",
                "build_type": "release",
                "dry_run": true
            }),
        )
        .unwrap();
        let rendered = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![
                "provenance",
                "--json",
                "attach-attestation",
                "--revision",
                "heads/main",
                "--file",
                "attestation.json",
                "--subject-digest",
                "sha256:abc",
                "--builder-id",
                "builder-a",
                "--build-type",
                "release",
                "--dry-run",
            ]
        );
    }

    #[test]
    fn rejects_missing_required_argument() {
        let err = tool_command("claw_change_create", &json!({})).unwrap_err();
        assert_eq!(err.0, -32602);
        assert!(err.1.contains("intent_id"));
    }
}
