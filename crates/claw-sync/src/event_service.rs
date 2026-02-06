use std::sync::Arc;

use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

use claw_store::ClawStore;

use crate::proto::event::event_stream_service_server::EventStreamService;
use crate::proto::event::*;

pub struct EventServer {
    store: Arc<RwLock<ClawStore>>,
}

impl EventServer {
    pub fn new(store: Arc<RwLock<ClawStore>>) -> Self {
        Self { store }
    }
}

#[tonic::async_trait]
impl EventStreamService for EventServer {
    type SubscribeStream = tokio_stream::wrappers::ReceiverStream<Result<Event, Status>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let req = request.into_inner();
        let store = self.store.clone();
        let event_types = req.event_types;
        let ref_prefix = req.ref_prefix;

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            // Poll for ref changes at a fixed interval.
            // A production implementation would use filesystem watches or
            // an internal event bus; this polling approach is sufficient for MVP.
            let mut known_refs = std::collections::HashMap::new();

            // Seed with current refs
            {
                let s = store.read().await;
                if let Ok(refs) = s.list_refs(&ref_prefix) {
                    for (name, id) in refs {
                        known_refs.insert(name, id);
                    }
                }
            }

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                let current_refs = {
                    let s = store.read().await;
                    match s.list_refs(&ref_prefix) {
                        Ok(r) => r,
                        Err(_) => continue,
                    }
                };

                for (name, id) in &current_refs {
                    let changed = match known_refs.get(name) {
                        Some(old_id) => old_id != id,
                        None => true,
                    };

                    if changed {
                        let event_type = if known_refs.contains_key(name) {
                            "ref_updated"
                        } else {
                            "ref_created"
                        };

                        if !event_types.is_empty()
                            && !event_types.iter().any(|t| t == event_type)
                        {
                            known_refs.insert(name.clone(), *id);
                            continue;
                        }

                        let event = Event {
                            event_type: event_type.to_string(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64,
                            ref_name: name.clone(),
                            object_id: Some(crate::proto::common::ObjectId {
                                hash: id.as_bytes().to_vec(),
                            }),
                            message: format!("{event_type}: {name}"),
                        };

                        if tx.send(Ok(event)).await.is_err() {
                            return; // Client disconnected
                        }

                        known_refs.insert(name.clone(), *id);
                    }
                }

                // Detect deletions
                let current_names: std::collections::HashSet<_> =
                    current_refs.iter().map(|(n, _)| n.clone()).collect();
                let removed: Vec<_> = known_refs
                    .keys()
                    .filter(|k| !current_names.contains(*k))
                    .cloned()
                    .collect();

                for name in removed {
                    if !event_types.is_empty()
                        && !event_types.iter().any(|t| t == "ref_deleted")
                    {
                        known_refs.remove(&name);
                        continue;
                    }

                    let event = Event {
                        event_type: "ref_deleted".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        ref_name: name.clone(),
                        object_id: None,
                        message: format!("ref_deleted: {name}"),
                    };

                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }

                    known_refs.remove(&name);
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}
