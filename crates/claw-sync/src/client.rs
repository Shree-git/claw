use async_trait::async_trait;
use claw_core::cof::{cof_decode, cof_encode};
use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

use crate::http_client::HttpSyncClient;
use crate::proto::sync::sync_service_client::SyncServiceClient;
use crate::proto::sync::*;
use crate::transport::{RemoteTransportConfig, SyncTransport};
use crate::SyncError;

pub struct SyncClient {
    inner: Box<dyn SyncTransport>,
}

impl SyncClient {
    pub async fn connect(addr: &str) -> Result<Self, SyncError> {
        Self::connect_with_transport(RemoteTransportConfig::Grpc {
            addr: addr.to_string(),
        })
        .await
    }

    pub async fn connect_with_transport(config: RemoteTransportConfig) -> Result<Self, SyncError> {
        let inner: Box<dyn SyncTransport> = match config {
            RemoteTransportConfig::Grpc { addr } => Box::new(GrpcSyncClient::connect(&addr).await?),
            RemoteTransportConfig::Http {
                base_url,
                repo,
                bearer_token,
            } => Box::new(HttpSyncClient::new(base_url, repo, bearer_token)),
        };

        Ok(Self { inner })
    }

    pub async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
        self.inner.hello().await
    }

    pub async fn advertise_refs(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, ObjectId)>, SyncError> {
        self.inner.advertise_refs(prefix).await
    }

    pub async fn fetch_objects(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError> {
        self.inner.fetch_objects(store, want, have).await
    }

    pub async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError> {
        self.inner.update_refs(updates, force).await
    }

    pub async fn push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        self.inner.push_objects(store, ids).await
    }
}

pub struct GrpcSyncClient {
    client: SyncServiceClient<tonic::transport::Channel>,
}

impl GrpcSyncClient {
    pub async fn connect(addr: &str) -> Result<Self, SyncError> {
        let client = SyncServiceClient::connect(addr.to_string()).await?;
        Ok(Self { client })
    }
}

#[async_trait]
impl SyncTransport for GrpcSyncClient {
    async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
        let resp = self
            .client
            .hello(HelloRequest {
                client_version: "0.1.0".to_string(),
                capabilities: vec!["partial-clone".to_string()],
            })
            .await?;
        Ok(resp.into_inner())
    }

    async fn advertise_refs(&mut self, prefix: &str) -> Result<Vec<(String, ObjectId)>, SyncError> {
        let resp = self
            .client
            .advertise_refs(AdvertiseRefsRequest {
                prefix: prefix.to_string(),
            })
            .await?;
        let inner = resp.into_inner();

        let mut refs = Vec::new();
        for entry in inner.refs {
            if let Some(id_msg) = entry.target {
                let id_bytes: [u8; 32] = id_msg
                    .hash
                    .as_slice()
                    .try_into()
                    .map_err(|_| SyncError::NegotiationFailed("invalid object id".into()))?;
                refs.push((entry.name, ObjectId::from_bytes(id_bytes)));
            }
        }
        Ok(refs)
    }

    async fn fetch_objects(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError> {
        let want_msgs: Vec<_> = want
            .iter()
            .map(|id| crate::proto::common::ObjectId {
                hash: id.as_bytes().to_vec(),
            })
            .collect();
        let have_msgs: Vec<_> = have
            .iter()
            .map(|id| crate::proto::common::ObjectId {
                hash: id.as_bytes().to_vec(),
            })
            .collect();

        let resp = self
            .client
            .fetch_objects(FetchObjectsRequest {
                want: want_msgs,
                have: have_msgs,
                filter: None,
            })
            .await?;

        let mut stream = resp.into_inner();
        let mut fetched = Vec::new();

        while let Some(chunk) = stream.message().await? {
            if chunk.is_last {
                break;
            }
            let (type_tag, payload) = cof_decode(&chunk.data)?;
            let obj = Object::deserialize_payload(type_tag, &payload)?;
            let id = store.store_object(&obj)?;
            fetched.push(id);
        }

        Ok(fetched)
    }

    async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError> {
        let proto_updates: Vec<RefUpdate> = updates
            .iter()
            .map(|(name, old, new)| RefUpdate {
                name: name.clone(),
                old_target: old.map(|id| crate::proto::common::ObjectId {
                    hash: id.as_bytes().to_vec(),
                }),
                new_target: Some(crate::proto::common::ObjectId {
                    hash: new.as_bytes().to_vec(),
                }),
                force,
            })
            .collect();

        let resp = self
            .client
            .update_refs(UpdateRefsRequest {
                updates: proto_updates,
            })
            .await?;
        Ok(resp.into_inner())
    }

    async fn push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let mut chunks = Vec::new();

        for id in ids {
            let obj = store.load_object(id)?;
            let payload = obj.serialize_payload()?;
            let type_tag = obj.type_tag();
            let cof_data = cof_encode(type_tag, &payload)?;

            chunks.push(ObjectChunk {
                id: Some(crate::proto::common::ObjectId {
                    hash: id.as_bytes().to_vec(),
                }),
                object_type: type_tag as i32,
                data: cof_data,
                is_last: false,
            });
        }

        chunks.push(ObjectChunk {
            id: None,
            object_type: 0,
            data: vec![],
            is_last: true,
        });

        let stream = tokio_stream::iter(chunks);
        let resp = self.client.push_objects(stream).await?;
        Ok(resp.into_inner())
    }
}
