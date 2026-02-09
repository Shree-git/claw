use std::sync::Arc;

use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

use claw_core::cof::cof_encode;
use claw_core::id::ObjectId;
use claw_store::ClawStore;

use crate::ancestry::is_ancestor;
use crate::negotiation::find_reachable_objects;
use crate::partial_clone::PartialCloneFilter;
use crate::proto::sync::sync_service_server::SyncService;
use crate::proto::sync::*;

pub struct SyncServer {
    store: Arc<RwLock<ClawStore>>,
}

impl SyncServer {
    pub fn new(store: ClawStore) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
        }
    }

    pub fn from_shared(store: Arc<RwLock<ClawStore>>) -> Self {
        Self { store }
    }
}

/// Convert a proto PartialCloneFilter to our internal filter type.
fn convert_filter(filter: &crate::proto::sync::PartialCloneFilter) -> PartialCloneFilter {
    PartialCloneFilter {
        path_prefixes: filter.path_prefixes.clone(),
        codec_ids: filter.codec_ids.clone(),
        time_range: if filter.time_range_start > 0 || filter.time_range_end > 0 {
            Some((filter.time_range_start, filter.time_range_end))
        } else {
            None
        },
        max_depth: if filter.max_depth > 0 {
            Some(filter.max_depth)
        } else {
            None
        },
        max_bytes: if filter.max_bytes > 0 {
            Some(filter.max_bytes)
        } else {
            None
        },
    }
}

fn decode_object_id(msg: &crate::proto::common::ObjectId) -> Result<ObjectId, Status> {
    let id_bytes: [u8; 32] = msg
        .hash
        .as_slice()
        .try_into()
        .map_err(|_| Status::invalid_argument("invalid object id"))?;
    Ok(ObjectId::from_bytes(id_bytes))
}

#[tonic::async_trait]
impl SyncService for SyncServer {
    async fn hello(&self, request: Request<HelloRequest>) -> Result<Response<HelloResponse>, Status> {
        let _req = request.into_inner();
        Ok(Response::new(HelloResponse {
            server_version: "0.1.0".to_string(),
            capabilities: vec!["partial-clone".to_string()],
        }))
    }

    async fn advertise_refs(
        &self,
        request: Request<AdvertiseRefsRequest>,
    ) -> Result<Response<AdvertiseRefsResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.read().await;
        let refs = store
            .list_refs(&req.prefix)
            .map_err(|e| Status::internal(e.to_string()))?;

        let entries = refs
            .into_iter()
            .map(|(name, id)| RefEntry {
                name,
                target: Some(crate::proto::common::ObjectId {
                    hash: id.as_bytes().to_vec(),
                }),
            })
            .collect();

        Ok(Response::new(AdvertiseRefsResponse { refs: entries }))
    }

    type FetchObjectsStream =
        tokio_stream::wrappers::ReceiverStream<Result<ObjectChunk, Status>>;

    async fn fetch_objects(
        &self,
        request: Request<FetchObjectsRequest>,
    ) -> Result<Response<Self::FetchObjectsStream>, Status> {
        let req = request.into_inner();
        let store = self.store.clone();
        let filter = req.filter.as_ref().map(convert_filter);

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let store = store.read().await;

            // Compute want_set = reachable from want_ids
            let want_ids: Vec<ObjectId> = req
                .want
                .iter()
                .filter_map(|msg| {
                    let bytes: [u8; 32] = msg.hash.as_slice().try_into().ok()?;
                    Some(ObjectId::from_bytes(bytes))
                })
                .collect();

            let have_ids: Vec<ObjectId> = req
                .have
                .iter()
                .filter_map(|msg| {
                    let bytes: [u8; 32] = msg.hash.as_slice().try_into().ok()?;
                    Some(ObjectId::from_bytes(bytes))
                })
                .collect();

            let want_set = find_reachable_objects(&store, &want_ids);
            let have_set = find_reachable_objects(&store, &have_ids);

            // Send want_set - have_set
            for id in &want_set {
                if have_set.contains(id) {
                    continue;
                }

                // Apply partial clone filter if present
                if let Some(ref f) = filter {
                    if !f.matches_object(&store, id) {
                        continue;
                    }
                }

                if let Ok(obj) = store.load_object(id) {
                    let payload = obj.serialize_payload().unwrap_or_default();
                    let type_tag = obj.type_tag();
                    let cof_data = cof_encode(type_tag, &payload).unwrap_or_default();

                    let chunk = ObjectChunk {
                        id: Some(crate::proto::common::ObjectId {
                            hash: id.as_bytes().to_vec(),
                        }),
                        object_type: type_tag as i32,
                        data: cof_data,
                        is_last: false,
                    };
                    if tx.send(Ok(chunk)).await.is_err() {
                        break;
                    }
                }
            }
            // Send final marker
            let _ = tx
                .send(Ok(ObjectChunk {
                    id: None,
                    object_type: 0,
                    data: vec![],
                    is_last: true,
                }))
                .await;
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn push_objects(
        &self,
        request: Request<tonic::Streaming<ObjectChunk>>,
    ) -> Result<Response<PushObjectsResponse>, Status> {
        let mut stream = request.into_inner();
        let store = self.store.write().await;
        let mut accepted = Vec::new();

        while let Some(chunk) = stream.message().await? {
            if chunk.is_last {
                break;
            }

            if let Some(_id_msg) = &chunk.id {
                let (type_tag, payload) = claw_core::cof::cof_decode(&chunk.data)
                    .map_err(|e| Status::internal(e.to_string()))?;
                let obj = claw_core::object::Object::deserialize_payload(type_tag, &payload)
                    .map_err(|e| Status::internal(e.to_string()))?;
                let id = store
                    .store_object(&obj)
                    .map_err(|e| Status::internal(e.to_string()))?;
                accepted.push(crate::proto::common::ObjectId {
                    hash: id.as_bytes().to_vec(),
                });
            }
        }

        Ok(Response::new(PushObjectsResponse {
            success: true,
            message: format!("accepted {} objects", accepted.len()),
            accepted,
        }))
    }

    async fn update_refs(
        &self,
        request: Request<UpdateRefsRequest>,
    ) -> Result<Response<UpdateRefsResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.write().await;

        // Two-pass: first verify all CAS conditions, then apply
        // Pass 1: verify
        for update in &req.updates {
            let current = store
                .get_ref(&update.name)
                .map_err(|e| Status::internal(e.to_string()))?;

            let expected_old = update
                .old_target
                .as_ref()
                .map(|msg| decode_object_id(msg))
                .transpose()?;

            match (&expected_old, &current) {
                (None, None) => {} // Creating new ref
                (Some(expected), Some(actual)) if expected == actual => {}
                (None, Some(_)) if update.force => {} // Force override existing ref
                _ => {
                    return Ok(Response::new(UpdateRefsResponse {
                        success: false,
                        message: format!(
                            "CAS conflict on ref '{}': expected {:?}, actual {:?}",
                            update.name,
                            expected_old.map(|id| id.to_hex()),
                            current.map(|id| id.to_hex()),
                        ),
                    }));
                }
            }

            // FF check: verify new is descendant of old (unless force)
            if let Some(new_target) = &update.new_target {
                let new_id = decode_object_id(new_target)?;
                if let Some(ref old_id) = current {
                    if !update.force && !is_ancestor(&store, old_id, &new_id) {
                        return Ok(Response::new(UpdateRefsResponse {
                            success: false,
                            message: format!(
                                "non-fast-forward update on ref '{}'; use force to override",
                                update.name
                            ),
                        }));
                    }
                }
            }
        }

        // Pass 2: apply all updates
        for update in &req.updates {
            if let Some(new_target) = &update.new_target {
                let id = decode_object_id(new_target)?;
                store
                    .set_ref(&update.name, &id)
                    .map_err(|e| Status::internal(e.to_string()))?;
            }
        }

        Ok(Response::new(UpdateRefsResponse {
            success: true,
            message: "refs updated".to_string(),
        }))
    }
}
