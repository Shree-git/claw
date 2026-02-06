use std::sync::Arc;

use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

use claw_core::id::ChangeId;
use claw_core::object::Object;
use claw_core::types::Workstream;
use claw_store::ClawStore;

use crate::proto::workstream::workstream_service_server::WorkstreamService;
use crate::proto::workstream::*;

pub struct WorkstreamServer {
    store: Arc<RwLock<ClawStore>>,
}

impl WorkstreamServer {
    pub fn new(store: Arc<RwLock<ClawStore>>) -> Self {
        Self { store }
    }
}

fn workstream_to_proto(w: &Workstream) -> crate::proto::objects::Workstream {
    crate::proto::objects::Workstream {
        workstream_id: w.workstream_id.clone(),
        change_stack: w
            .change_stack
            .iter()
            .map(|c| crate::proto::common::Ulid {
                data: c.as_bytes().to_vec(),
            })
            .collect(),
    }
}

#[tonic::async_trait]
impl WorkstreamService for WorkstreamServer {
    async fn create(
        &self,
        request: Request<CreateWorkstreamRequest>,
    ) -> Result<Response<CreateWorkstreamResponse>, Status> {
        let req = request.into_inner();

        let ws = Workstream {
            workstream_id: req.name.clone(),
            change_stack: vec![],
        };

        let store = self.store.write().await;
        let obj_id = store
            .store_object(&Object::Workstream(ws.clone()))
            .map_err(|e| Status::internal(e.to_string()))?;
        store
            .set_ref(&format!("workstreams/{}", req.name), &obj_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CreateWorkstreamResponse {
            workstream: Some(workstream_to_proto(&ws)),
        }))
    }

    async fn get(
        &self,
        request: Request<GetWorkstreamRequest>,
    ) -> Result<Response<GetWorkstreamResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.read().await;

        let obj_id = store
            .get_ref(&format!("workstreams/{}", req.name))
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("workstream not found"))?;

        match store
            .load_object(&obj_id)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            Object::Workstream(w) => Ok(Response::new(GetWorkstreamResponse {
                workstream: Some(workstream_to_proto(&w)),
            })),
            _ => Err(Status::internal("object is not a workstream")),
        }
    }

    async fn push_change(
        &self,
        request: Request<PushChangeRequest>,
    ) -> Result<Response<PushChangeResponse>, Status> {
        let req = request.into_inner();
        let ulid = req
            .change_id
            .ok_or_else(|| Status::invalid_argument("missing change_id"))?;
        let arr: [u8; 16] = ulid
            .data
            .as_slice()
            .try_into()
            .map_err(|_| Status::invalid_argument("invalid ULID"))?;
        let change_id = ChangeId::from_bytes(arr);

        let store = self.store.write().await;
        let ref_name = format!("workstreams/{}", req.workstream_name);
        let obj_id = store
            .get_ref(&ref_name)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("workstream not found"))?;

        let mut ws = match store
            .load_object(&obj_id)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            Object::Workstream(w) => w,
            _ => return Err(Status::internal("not a workstream")),
        };

        ws.change_stack.push(change_id);

        let new_obj_id = store
            .store_object(&Object::Workstream(ws))
            .map_err(|e| Status::internal(e.to_string()))?;
        store
            .set_ref(&ref_name, &new_obj_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(PushChangeResponse { success: true }))
    }

    async fn pop_change(
        &self,
        request: Request<PopChangeRequest>,
    ) -> Result<Response<PopChangeResponse>, Status> {
        let req = request.into_inner();
        let store = self.store.write().await;
        let ref_name = format!("workstreams/{}", req.workstream_name);
        let obj_id = store
            .get_ref(&ref_name)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("workstream not found"))?;

        let mut ws = match store
            .load_object(&obj_id)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            Object::Workstream(w) => w,
            _ => return Err(Status::internal("not a workstream")),
        };

        let popped = ws
            .change_stack
            .pop()
            .ok_or_else(|| Status::failed_precondition("workstream stack is empty"))?;

        let new_obj_id = store
            .store_object(&Object::Workstream(ws))
            .map_err(|e| Status::internal(e.to_string()))?;
        store
            .set_ref(&ref_name, &new_obj_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(PopChangeResponse {
            change_id: Some(crate::proto::common::Ulid {
                data: popped.as_bytes().to_vec(),
            }),
        }))
    }
}
