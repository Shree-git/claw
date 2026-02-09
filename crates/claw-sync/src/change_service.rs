use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

use claw_core::id::{ChangeId, IntentId};
use claw_core::object::Object;
use claw_core::types::{Change, ChangeStatus};
use claw_store::ClawStore;

use crate::proto::change::change_service_server::ChangeService;
use crate::proto::change::*;

pub struct ChangeServer {
    store: Arc<RwLock<ClawStore>>,
}

impl ChangeServer {
    pub fn new(store: Arc<RwLock<ClawStore>>) -> Self {
        Self { store }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn change_to_proto(c: &Change) -> crate::proto::objects::Change {
    crate::proto::objects::Change {
        id: Some(crate::proto::common::Ulid {
            data: c.id.as_bytes().to_vec(),
        }),
        intent_id: Some(crate::proto::common::Ulid {
            data: c.intent_id.as_bytes().to_vec(),
        }),
        head_revision: c.head_revision.map(|id| crate::proto::common::ObjectId {
            hash: id.as_bytes().to_vec(),
        }),
        workstream_id: c.workstream_id.clone().unwrap_or_default(),
        status: match c.status {
            ChangeStatus::Open => "open".into(),
            ChangeStatus::Ready => "ready".into(),
            ChangeStatus::Integrated => "integrated".into(),
            ChangeStatus::Abandoned => "abandoned".into(),
        },
        created_at_ms: c.created_at_ms,
        updated_at_ms: c.updated_at_ms,
    }
}

#[allow(clippy::result_large_err)]
fn parse_status(s: &str) -> Result<ChangeStatus, Status> {
    match s {
        "open" => Ok(ChangeStatus::Open),
        "ready" => Ok(ChangeStatus::Ready),
        "integrated" => Ok(ChangeStatus::Integrated),
        "abandoned" => Ok(ChangeStatus::Abandoned),
        _ => Err(Status::invalid_argument(format!("invalid status: {s}"))),
    }
}

#[tonic::async_trait]
impl ChangeService for ChangeServer {
    async fn create(
        &self,
        request: Request<CreateChangeRequest>,
    ) -> Result<Response<CreateChangeResponse>, Status> {
        let req = request.into_inner();
        let now = now_ms();

        let intent_ulid = req
            .intent_id
            .ok_or_else(|| Status::invalid_argument("missing intent_id"))?;
        let arr: [u8; 16] = intent_ulid
            .data
            .as_slice()
            .try_into()
            .map_err(|_| Status::invalid_argument("invalid intent ULID"))?;
        let intent_id = IntentId::from_bytes(arr);

        let id = ChangeId::new();
        let change = Change {
            id,
            intent_id,
            head_revision: None,
            workstream_id: None,
            status: ChangeStatus::Open,
            created_at_ms: now,
            updated_at_ms: now,
        };

        let store = self.store.write().await;
        let obj_id = store
            .store_object(&Object::Change(change.clone()))
            .map_err(|e| Status::internal(e.to_string()))?;
        store
            .set_ref(&format!("changes/{id}"), &obj_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CreateChangeResponse {
            change: Some(change_to_proto(&change)),
        }))
    }

    async fn get(
        &self,
        request: Request<GetChangeRequest>,
    ) -> Result<Response<GetChangeResponse>, Status> {
        let req = request.into_inner();
        let ulid = req
            .id
            .ok_or_else(|| Status::invalid_argument("missing id"))?;
        let arr: [u8; 16] = ulid
            .data
            .as_slice()
            .try_into()
            .map_err(|_| Status::invalid_argument("invalid ULID"))?;
        let change_id = ChangeId::from_bytes(arr);

        let store = self.store.read().await;
        let obj_id = store
            .get_ref(&format!("changes/{change_id}"))
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("change not found"))?;

        match store
            .load_object(&obj_id)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            Object::Change(c) => Ok(Response::new(GetChangeResponse {
                change: Some(change_to_proto(&c)),
            })),
            _ => Err(Status::internal("object is not a change")),
        }
    }

    async fn list(
        &self,
        request: Request<ListChangesRequest>,
    ) -> Result<Response<ListChangesResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.read().await;
        let refs = store
            .list_refs("changes")
            .map_err(|e| Status::internal(e.to_string()))?;

        let filter_intent: Option<[u8; 16]> = req
            .intent_id
            .as_ref()
            .and_then(|u| u.data.as_slice().try_into().ok());

        let mut changes = Vec::new();
        for (_, obj_id) in &refs {
            if let Ok(Object::Change(c)) = store.load_object(obj_id) {
                if let Some(fi) = &filter_intent {
                    if c.intent_id.as_bytes() != *fi {
                        continue;
                    }
                }
                if req.status_filter.is_empty() || status_matches(&c.status, &req.status_filter) {
                    changes.push(change_to_proto(&c));
                }
            }
        }

        Ok(Response::new(ListChangesResponse { changes }))
    }

    async fn update_status(
        &self,
        request: Request<UpdateChangeStatusRequest>,
    ) -> Result<Response<UpdateChangeStatusResponse>, Status> {
        let req = request.into_inner();
        let ulid = req
            .id
            .ok_or_else(|| Status::invalid_argument("missing id"))?;
        let arr: [u8; 16] = ulid
            .data
            .as_slice()
            .try_into()
            .map_err(|_| Status::invalid_argument("invalid ULID"))?;
        let change_id = ChangeId::from_bytes(arr);

        let store = self.store.write().await;
        let obj_id = store
            .get_ref(&format!("changes/{change_id}"))
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("change not found"))?;

        let mut change = match store
            .load_object(&obj_id)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            Object::Change(c) => c,
            _ => return Err(Status::internal("not a change")),
        };

        change.status = parse_status(&req.status)?;
        change.updated_at_ms = now_ms();

        let new_obj_id = store
            .store_object(&Object::Change(change.clone()))
            .map_err(|e| Status::internal(e.to_string()))?;
        store
            .set_ref(&format!("changes/{change_id}"), &new_obj_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(UpdateChangeStatusResponse {
            change: Some(change_to_proto(&change)),
        }))
    }
}

fn status_matches(s: &ChangeStatus, filter: &str) -> bool {
    matches!(
        (s, filter),
        (ChangeStatus::Open, "open")
            | (ChangeStatus::Ready, "ready")
            | (ChangeStatus::Integrated, "integrated")
            | (ChangeStatus::Abandoned, "abandoned")
    )
}
