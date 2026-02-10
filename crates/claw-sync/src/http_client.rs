use async_trait::async_trait;
use base64::prelude::*;
use claw_core::cof::{cof_decode, cof_encode};
use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::proto;
use crate::proto::sync::{HelloResponse, PushObjectsResponse, UpdateRefsResponse};
use crate::transport::SyncTransport;
use crate::SyncError;

#[derive(Debug, Clone)]
pub struct HttpSyncClient {
    base_url: String,
    repo: String,
    bearer_token: Option<String>,
    client: reqwest::Client,
}

impl HttpSyncClient {
    pub fn new(base_url: String, repo: String, bearer_token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            repo,
            bearer_token,
            client: reqwest::Client::new(),
        }
    }

    fn endpoint(&self, suffix: &str) -> String {
        let repo = urlencoding::encode(&self.repo);
        format!("{}/v1/repos/{}{}", self.base_url, repo, suffix)
    }

    fn request(&self, method: reqwest::Method, url: String) -> reqwest::RequestBuilder {
        let mut builder = self.client.request(method.clone(), url);

        // ClawLab requires an idempotency key for mutating requests.
        if matches!(
            method,
            reqwest::Method::POST
                | reqwest::Method::PUT
                | reqwest::Method::PATCH
                | reqwest::Method::DELETE
        ) {
            let mut bytes = [0_u8; 16];
            rand::thread_rng().fill_bytes(&mut bytes);
            let key = BASE64_URL_SAFE_NO_PAD.encode(bytes);
            builder = builder.header("idempotency-key", key);
        }

        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }

        builder
    }
}

#[derive(Debug, Deserialize)]
struct RefsResponse {
    refs: Vec<HttpRef>,
}

#[derive(Debug, Deserialize)]
struct HttpRef {
    name: String,
    target: String,
}

#[derive(Debug, Serialize)]
struct RefUpdatePayload {
    name: String,
    #[serde(rename = "oldTarget")]
    old_target: Option<String>,
    #[serde(rename = "newTarget")]
    new_target: String,
    force: bool,
}

#[derive(Debug, Serialize)]
struct CasUpdateRequest {
    updates: Vec<RefUpdatePayload>,
}

#[derive(Debug, Deserialize)]
struct CasUpdateResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct UploadObject {
    #[serde(rename = "objectId")]
    object_id: String,
    #[serde(rename = "typeTag")]
    type_tag: i32,
    #[serde(rename = "cofBase64")]
    cof_base64: String,
    refs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UploadRequest {
    objects: Vec<UploadObject>,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    accepted: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DownloadRequest {
    want: Vec<String>,
    have: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DownloadEnvelope {
    objects: Vec<DownloadObject>,
}

#[derive(Debug, Deserialize)]
struct DownloadObject {
    #[serde(rename = "objectId")]
    object_id: String,
    #[serde(rename = "cofBase64")]
    cof_base64: String,
}

#[async_trait]
impl SyncTransport for HttpSyncClient {
    async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
        let url = format!("{}/health", self.base_url);
        let resp = self.request(reqwest::Method::GET, url).send().await?;
        if !resp.status().is_success() {
            return Err(SyncError::ConnectionFailed(format!(
                "health check failed: {}",
                resp.status()
            )));
        }

        Ok(HelloResponse {
            server_version: "clawlab-http-v1".to_string(),
            capabilities: vec!["partial-clone".to_string(), "polling-events".to_string()],
        })
    }

    async fn advertise_refs(&mut self, prefix: &str) -> Result<Vec<(String, ObjectId)>, SyncError> {
        let url = self.endpoint(&format!("/refs?prefix={}", urlencoding::encode(prefix)));
        let resp = self.request(reqwest::Method::GET, url).send().await?;
        if !resp.status().is_success() {
            return Err(SyncError::NegotiationFailed(format!(
                "advertise refs failed: {}",
                resp.status()
            )));
        }

        let body: RefsResponse = resp.json().await?;
        let mut refs = Vec::new();
        for entry in body.refs {
            let id = ObjectId::from_hex(&entry.target)
                .map_err(|e| SyncError::NegotiationFailed(format!("invalid object id: {e}")))?;
            refs.push((entry.name, id));
        }
        Ok(refs)
    }

    async fn fetch_objects(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError> {
        let url = self.endpoint("/objects:batch-download");
        let payload = DownloadRequest {
            want: want.iter().map(ObjectId::to_hex).collect(),
            have: have.iter().map(ObjectId::to_hex).collect(),
        };

        let resp = self
            .request(reqwest::Method::POST, url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(SyncError::TransferFailed(format!(
                "batch download failed: {}",
                resp.status()
            )));
        }

        let body: DownloadEnvelope = resp.json().await?;
        let mut fetched = Vec::new();

        for item in body.objects {
            let object_id = item.object_id.clone();
            let cof_bytes = BASE64_STANDARD.decode(item.cof_base64).map_err(|e| {
                SyncError::TransferFailed(format!("base64 decode failed for {object_id}: {e}"))
            })?;

            let (type_tag, payload) = cof_decode(&cof_bytes)?;
            let object = Object::deserialize_payload(type_tag, &payload)?;
            let id = store.store_object(&object)?;
            fetched.push(id);
        }

        Ok(fetched)
    }

    async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError> {
        let url = self.endpoint("/refs:cas-update");
        let payload = CasUpdateRequest {
            updates: updates
                .iter()
                .map(|(name, old, new)| RefUpdatePayload {
                    name: name.clone(),
                    old_target: old.map(|id| id.to_hex()),
                    new_target: new.to_hex(),
                    force,
                })
                .collect(),
        };

        let resp = self
            .request(reqwest::Method::POST, url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(SyncError::TransferFailed(format!(
                "cas update failed: {}",
                resp.status()
            )));
        }

        let body: CasUpdateResponse = resp.json().await?;
        Ok(UpdateRefsResponse {
            success: body.success,
            message: body.message,
        })
    }

    async fn push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let mut objects = Vec::new();

        for id in ids {
            let object = store.load_object(id)?;
            let payload = object.serialize_payload()?;
            let type_tag = object.type_tag();
            let cof_data = cof_encode(type_tag, &payload)?;
            let refs = object.dependencies().iter().map(ObjectId::to_hex).collect();

            objects.push(UploadObject {
                object_id: id.to_hex(),
                type_tag: type_tag as i32,
                cof_base64: BASE64_STANDARD.encode(cof_data),
                refs,
            });
        }

        let url = self.endpoint("/objects:batch-upload");
        let payload = UploadRequest { objects };
        let resp = self
            .request(reqwest::Method::POST, url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(SyncError::TransferFailed(format!(
                "batch upload failed: {}",
                resp.status()
            )));
        }

        let body: UploadResponse = resp.json().await?;
        let accepted = body
            .accepted
            .into_iter()
            .filter_map(|hex| ObjectId::from_hex(&hex).ok())
            .map(|id| proto::common::ObjectId {
                hash: id.as_bytes().to_vec(),
            })
            .collect();

        Ok(PushObjectsResponse {
            success: true,
            message: format!("accepted {} objects", ids.len()),
            accepted,
        })
    }
}
