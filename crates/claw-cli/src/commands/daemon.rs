use std::sync::Arc;

use clap::Args;
use tokio::sync::RwLock;

use claw_store::ClawStore;
use claw_sync::capsule_service::CapsuleServer;
use claw_sync::change_service::ChangeServer;
use claw_sync::event_service::EventServer;
use claw_sync::intent_service::IntentServer;
use claw_sync::proto::capsule::capsule_service_server::CapsuleServiceServer;
use claw_sync::proto::change::change_service_server::ChangeServiceServer;
use claw_sync::proto::event::event_stream_service_server::EventStreamServiceServer;
use claw_sync::proto::intent::intent_service_server::IntentServiceServer;
use claw_sync::proto::sync::sync_service_server::SyncServiceServer;
use claw_sync::proto::workstream::workstream_service_server::WorkstreamServiceServer;
use claw_sync::server::SyncServer;
use claw_sync::workstream_service::WorkstreamServer;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct DaemonArgs {
    /// Listen address
    #[arg(short, long, default_value = "[::1]:50051")]
    listen: String,
    /// Use stdio instead of TCP (for embedded use)
    #[arg(long)]
    stdio: bool,
}

pub async fn run(args: DaemonArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;

    if args.stdio {
        // Stdio mode: read/write framed messages on stdin/stdout.
        // For embedded agent use. Uses length-prefixed JSON frames.
        eprintln!("Claw daemon running in stdio mode");
        eprintln!("Send JSON-RPC requests on stdin, receive responses on stdout");

        use tokio::io::{AsyncBufReadExt, BufReader};
        let stdin = BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();

        while let Some(line) = lines.next_line().await? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            // Simple echo-based protocol for MVP
            let response = match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(req) => {
                    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
                    match method {
                        "hello" => serde_json::json!({
                            "server_version": "0.1.0",
                            "capabilities": ["partial-clone"]
                        }),
                        "refs" => {
                            let prefix = req.get("prefix").and_then(|p| p.as_str()).unwrap_or("");
                            match store.list_refs(prefix) {
                                Ok(refs) => {
                                    let r: Vec<_> = refs.iter()
                                        .map(|(name, id)| serde_json::json!({"name": name, "target": id.to_string()}))
                                        .collect();
                                    serde_json::json!({"refs": r})
                                }
                                Err(e) => serde_json::json!({"error": e.to_string()}),
                            }
                        }
                        _ => serde_json::json!({"error": format!("unknown method: {method}")}),
                    }
                }
                Err(e) => serde_json::json!({"error": e.to_string()}),
            };
            println!("{}", serde_json::to_string(&response)?);
        }
        return Ok(());
    }

    let addr = args.listen.parse()?;
    let shared_store = Arc::new(RwLock::new(store));

    let sync_server = SyncServer::from_shared(shared_store.clone());
    let intent_server = IntentServer::new(shared_store.clone());
    let change_server = ChangeServer::new(shared_store.clone());
    let capsule_server = CapsuleServer::new(shared_store.clone());
    let workstream_server = WorkstreamServer::new(shared_store.clone());
    let event_server = EventServer::new(shared_store);

    println!("Claw daemon listening on {}", addr);
    tonic::transport::Server::builder()
        .add_service(SyncServiceServer::new(sync_server))
        .add_service(IntentServiceServer::new(intent_server))
        .add_service(ChangeServiceServer::new(change_server))
        .add_service(CapsuleServiceServer::new(capsule_server))
        .add_service(WorkstreamServiceServer::new(workstream_server))
        .add_service(EventStreamServiceServer::new(event_server))
        .serve(addr)
        .await?;

    Ok(())
}
