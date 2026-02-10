use async_trait::async_trait;

use claw_core::id::ObjectId;
use claw_store::ClawStore;

use crate::proto::sync::{HelloResponse, PushObjectsResponse, UpdateRefsResponse};
use crate::SyncError;

#[derive(Debug, Clone)]
pub enum RemoteTransportConfig {
    Grpc {
        addr: String,
    },
    Http {
        base_url: String,
        repo: String,
        bearer_token: Option<String>,
    },
}

#[async_trait]
pub trait SyncTransport: Send {
    async fn hello(&mut self) -> Result<HelloResponse, SyncError>;

    async fn advertise_refs(&mut self, prefix: &str) -> Result<Vec<(String, ObjectId)>, SyncError>;

    async fn fetch_objects(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError>;

    async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError>;

    async fn push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError>;
}
