use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use base64::prelude::*;
use claw_core::cof::{cof_decode, cof_encode};
use claw_core::id::ObjectId;
use claw_core::object::{Object, TypeTag};
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
    health_checked: bool,
    server_version: Option<String>,
    server_capabilities: HashSet<String>,
    capabilities_advertised: bool,
}

// Keep transfers under Vercel's hard request/response size limits.
const OBJECT_BYTES_CHUNK_SIZE: usize = 4_000_000;
const INLINE_OBJECT_MAX_BYTES: usize = 1_000_000;
const INLINE_BATCH_MAX_BYTES: usize = 2_500_000;
const CAP_CHUNKED_OBJECTS_V1: &str = "chunked-objects-v1";
const MAX_CONCURRENT_UPLOADS: usize = 8;

impl HttpSyncClient {
    pub fn new(base_url: String, repo: String, bearer_token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            repo,
            bearer_token,
            client: reqwest::Client::new(),
            health_checked: false,
            server_version: None,
            server_capabilities: HashSet::new(),
            capabilities_advertised: false,
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

    async fn ensure_health(&mut self) -> Result<(), SyncError> {
        if self.health_checked {
            return Ok(());
        }

        let url = format!("{}/health", self.base_url);
        let resp = self.request(reqwest::Method::GET, url).send().await?;
        if !resp.status().is_success() {
            return Err(SyncError::ConnectionFailed(format!(
                "health check failed: {}",
                resp.status()
            )));
        }

        let health: HealthResponse = resp.json().await?;
        self.server_version =
            Some(health.server_version.unwrap_or_else(|| "clawlab-http-v1".to_string()));
        self.capabilities_advertised = health.capabilities.is_some();
        self.server_capabilities = health.capabilities.unwrap_or_default().into_iter().collect();

        // Older servers may not advertise capabilities; assume a minimal baseline.
        if self.server_capabilities.is_empty() && !self.capabilities_advertised {
            self.server_capabilities
                .insert("partial-clone".to_string());
            self.server_capabilities
                .insert("polling-events".to_string());
        }

        self.health_checked = true;
        Ok(())
    }

    async fn fetch_object_bytes(
        &self,
        object_id: &str,
        size_bytes: usize,
    ) -> Result<Vec<u8>, SyncError> {
        let url = self.endpoint(&format!(
            "/objects/{}:bytes",
            urlencoding::encode(object_id)
        ));
        let mut out = Vec::with_capacity(size_bytes);
        let mut start: usize = 0;

        while start < size_bytes {
            let end = std::cmp::min(start + OBJECT_BYTES_CHUNK_SIZE, size_bytes) - 1;
            let range = format!("bytes={}-{}", start, end);

            let resp = self
                .request(reqwest::Method::GET, url.clone())
                .header(reqwest::header::RANGE, range)
                .send()
                .await?;

            if !(resp.status().is_success()
                || resp.status() == reqwest::StatusCode::PARTIAL_CONTENT)
            {
                return Err(SyncError::TransferFailed(format!(
                    "object bytes download failed for {}: {}",
                    object_id,
                    resp.status()
                )));
            }

            let bytes = resp.bytes().await?;
            if bytes.is_empty() {
                return Err(SyncError::TransferFailed(format!(
                    "empty bytes response for {} at offset {}",
                    object_id, start
                )));
            }

            out.extend_from_slice(&bytes);
            start += bytes.len();
        }

        Ok(out)
    }

    async fn upload_object_chunks(
        &self,
        object_id: &str,
        upload_id: &str,
        cof_bytes: &[u8],
        chunk_size: usize,
        total_chunks: usize,
    ) -> Result<(), SyncError> {
        for idx in 0..total_chunks {
            let start = idx * chunk_size;
            let end = std::cmp::min(start + chunk_size, cof_bytes.len());
            let chunk = &cof_bytes[start..end];

            let url = self.endpoint(&format!(
                "/objects/{}/uploads/{}/chunks/{}",
                urlencoding::encode(object_id),
                urlencoding::encode(upload_id),
                idx
            ));

            let resp = self
                .request(reqwest::Method::PUT, url)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(chunk.to_vec())
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(SyncError::TransferFailed(format!(
                    "chunk upload failed for {} idx {}: {} body={}",
                    object_id,
                    idx,
                    status,
                    body
                )));
            }
        }

        let url = self.endpoint(&format!(
            "/objects/{}/uploads/{}:complete",
            urlencoding::encode(object_id),
            urlencoding::encode(upload_id)
        ));
        let resp = self.request(reqwest::Method::POST, url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::TransferFailed(format!(
                "upload complete failed for {}: {} body={}",
                object_id,
                status,
                body
            )));
        }

        Ok(())
    }

    async fn send_upload_batch(
        &self,
        url: &str,
        batch: Vec<UploadObject>,
        prepared_map: &std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<HashSet<ObjectId>, SyncError> {
        if batch.is_empty() {
            return Ok(HashSet::new());
        }

        let payload = UploadRequest { objects: batch };
        let resp = self
            .request(reqwest::Method::POST, url.to_string())
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::TransferFailed(format!(
                "batch upload failed: {} body={}",
                status,
                body
            )));
        }

        let body: UploadResponse = resp.json().await?;
        let mut accepted_ids = HashSet::new();
        for hex in body.accepted {
            if let Ok(id) = ObjectId::from_hex(&hex) {
                accepted_ids.insert(id);
            }
        }

        // Upload required objects concurrently.
        let uploaded = self
            .upload_required_objects(body.required_uploads, prepared_map)
            .await?;
        accepted_ids.extend(uploaded);

        Ok(accepted_ids)
    }

    async fn upload_required_objects(
        &self,
        required_uploads: Vec<RequiredUpload>,
        prepared_map: &std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<Vec<ObjectId>, SyncError> {
        if required_uploads.is_empty() {
            return Ok(Vec::new());
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS));
        let mut join_set = tokio::task::JoinSet::new();

        for required in required_uploads {
            let cof = prepared_map
                .get(&required.object_id)
                .ok_or_else(|| {
                    SyncError::TransferFailed(format!(
                        "missing prepared bytes for required upload {}",
                        required.object_id
                    ))
                })?
                .clone();

            let client = self.clone();
            let sem = semaphore.clone();

            join_set.spawn(async move {
                let _permit = sem.acquire().await.map_err(|_| {
                    SyncError::TransferFailed("semaphore closed".to_string())
                })?;
                client
                    .upload_object_chunks(
                        &required.object_id,
                        &required.upload_id,
                        &cof,
                        required.chunk_size,
                        required.total_chunks,
                    )
                    .await?;
                ObjectId::from_hex(&required.object_id).map_err(|e| {
                    SyncError::TransferFailed(format!("invalid object id: {e}"))
                })
            });
        }

        let mut uploaded_ids = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(id)) => uploaded_ids.push(id),
                Ok(Err(sync_err)) => return Err(sync_err),
                Err(join_err) => {
                    return Err(SyncError::TransferFailed(format!(
                        "upload task panicked: {join_err}"
                    )));
                }
            }
        }

        Ok(uploaded_ids)
    }

    async fn fetch_objects_chunked(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError> {
        let url = self.endpoint("/objects:batch-download");
        let mut fetched = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let payload = DownloadRequest {
                want: want.iter().map(ObjectId::to_hex).collect(),
                have: have.iter().map(ObjectId::to_hex).collect(),
                cursor: cursor.clone(),
                limit: Some(2000),
            };

            let resp = self
                .request(reqwest::Method::POST, url.clone())
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
            if body.objects.is_empty() {
                break;
            }

            for item in body.objects {
                let object_id = item.object_id.clone();
                let expected_id = ObjectId::from_hex(&object_id).map_err(|e| {
                    SyncError::TransferFailed(format!("invalid object id in manifest: {e}"))
                })?;
                let expected_type = TypeTag::from_u8(item.type_tag as u8).ok_or_else(|| {
                    SyncError::TransferFailed(format!(
                        "invalid type tag in manifest for {}: {}",
                        object_id, item.type_tag
                    ))
                })?;

                let cof_bytes = self.fetch_object_bytes(&object_id, item.size_bytes).await?;

                let (type_tag, payload) = cof_decode(&cof_bytes)?;
                if type_tag != expected_type {
                    return Err(SyncError::TransferFailed(format!(
                        "type tag mismatch for {}: manifest={} cof={}",
                        object_id,
                        expected_type.name(),
                        type_tag.name()
                    )));
                }

                let object = Object::deserialize_payload(type_tag, &payload)?;
                let id = store.store_object(&object)?;
                if id != expected_id {
                    return Err(SyncError::TransferFailed(format!(
                        "object id mismatch for {}: expected={} actual={}",
                        object_id,
                        expected_id.to_hex(),
                        id.to_hex()
                    )));
                }
                fetched.push(id);
            }

            if let Some(next) = body.next_cursor {
                cursor = Some(next);
                continue;
            }
            break;
        }

        Ok(fetched)
    }

    async fn push_objects_chunked(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let mut prepared: Vec<(String, i32, Vec<u8>)> = Vec::new();

        for id in ids {
            let object = store.load_object(id)?;
            let payload = object.serialize_payload()?;
            let type_tag = object.type_tag();
            let cof_data = cof_encode(type_tag, &payload)?;
            prepared.push((id.to_hex(), type_tag as i32, cof_data));
        }

        let prepared_map: Arc<std::collections::HashMap<String, Vec<u8>>> = Arc::new(
            prepared
                .iter()
                .map(|(hex, _, cof)| (hex.clone(), cof.clone()))
                .collect(),
        );

        let url = self.endpoint("/objects:batch-upload");

        // Build all batches upfront.
        let mut batches: Vec<Vec<UploadObject>> = Vec::new();
        let mut current_batch: Vec<UploadObject> = Vec::new();

        for (object_id, type_tag, cof_data) in &prepared {
            current_batch.push(UploadObject {
                object_id: object_id.clone(),
                type_tag: *type_tag,
                size_bytes: cof_data.len(),
                // In chunked transfer mode, always use upload sessions.
                // Mixing inline + chunked objects in the same request can violate
                // dependency ordering because inline dependency checks run before
                // chunked completions.
                cof_base64: None,
            });

            if current_batch.len() >= 500 {
                batches.push(std::mem::take(&mut current_batch));
            }
        }
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        // Send all batches concurrently with bounded parallelism.
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS));
        let mut join_set = tokio::task::JoinSet::new();

        for batch in batches {
            let client = self.clone();
            let url = url.clone();
            let map = prepared_map.clone();
            let sem = semaphore.clone();

            join_set.spawn(async move {
                let _permit = sem.acquire().await.map_err(|_| {
                    SyncError::TransferFailed("semaphore closed".to_string())
                })?;
                client.send_upload_batch(&url, batch, &*map).await
            });
        }

        let mut accepted_ids: HashSet<ObjectId> = HashSet::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(batch_accepted)) => accepted_ids.extend(batch_accepted),
                Ok(Err(sync_err)) => return Err(sync_err),
                Err(join_err) => {
                    return Err(SyncError::TransferFailed(format!(
                        "batch upload task panicked: {join_err}"
                    )));
                }
            }
        }

        let accepted: Vec<_> = accepted_ids
            .into_iter()
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

    async fn fetch_objects_legacy(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError> {
        let url = self.endpoint("/objects:batch-download");
        let payload = DownloadRequest {
            want: want.iter().map(ObjectId::to_hex).collect(),
            have: have.iter().map(ObjectId::to_hex).collect(),
            cursor: None,
            limit: None,
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

        let body: LegacyDownloadEnvelope = resp.json().await?;
        let mut fetched = Vec::new();

        for item in body.objects {
            let object_id = item.object_id.clone();
            let expected_id = ObjectId::from_hex(&object_id).map_err(|e| {
                SyncError::TransferFailed(format!("invalid object id in legacy download: {e}"))
            })?;
            let expected_type = TypeTag::from_u8(item.type_tag as u8).ok_or_else(|| {
                SyncError::TransferFailed(format!(
                    "invalid type tag in legacy download for {}: {}",
                    object_id, item.type_tag
                ))
            })?;

            let cof_bytes = BASE64_STANDARD
                .decode(item.cof_base64)
                .map_err(|e| SyncError::TransferFailed(format!("invalid cofBase64 for {object_id}: {e}")))?;

            let (type_tag, payload) = cof_decode(&cof_bytes)?;
            if type_tag != expected_type {
                return Err(SyncError::TransferFailed(format!(
                    "type tag mismatch for {}: manifest={} cof={}",
                    object_id,
                    expected_type.name(),
                    type_tag.name()
                )));
            }

            let object = Object::deserialize_payload(type_tag, &payload)?;
            let id = store.store_object(&object)?;
            if id != expected_id {
                return Err(SyncError::TransferFailed(format!(
                    "object id mismatch for {}: expected={} actual={}",
                    object_id,
                    expected_id.to_hex(),
                    id.to_hex()
                )));
            }
            fetched.push(id);
        }

        Ok(fetched)
    }

    async fn push_objects_legacy(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let mut prepared: Vec<(String, i32, Vec<u8>)> = Vec::new();

        for id in ids {
            let object = store.load_object(id)?;
            let payload = object.serialize_payload()?;
            let type_tag = object.type_tag();
            let cof_data = cof_encode(type_tag, &payload)?;
            prepared.push((id.to_hex(), type_tag as i32, cof_data));
        }

        let prepared_map: Arc<std::collections::HashMap<String, Vec<u8>>> = Arc::new(
            prepared
                .iter()
                .map(|(hex, _, cof)| (hex.clone(), cof.clone()))
                .collect(),
        );

        let url = self.endpoint("/objects:batch-upload");
        let all_ids: HashSet<ObjectId> = ids.iter().copied().collect();
        let mut accepted_ids: HashSet<ObjectId> = HashSet::new();
        let mut pending = prepared;

        // Send batches concurrently with retry for dependency ordering.
        // Objects are in topological order (parents first). When sent concurrently,
        // some children may arrive before their parents are stored. Retrying
        // unaccepted objects in subsequent rounds resolves this.
        const MAX_RETRIES: usize = 3;
        for round in 0..=MAX_RETRIES {
            if pending.is_empty() {
                break;
            }

            for (object_id, _, cof_data) in &pending {
                if cof_data.len() > INLINE_OBJECT_MAX_BYTES {
                    return Err(SyncError::TransferFailed(format!(
                        "server does not advertise {CAP_CHUNKED_OBJECTS_V1}; \
                         cannot push large object {object_id} ({} bytes)",
                        cof_data.len()
                    )));
                }
            }

            // Build batches from pending objects.
            let mut batches: Vec<Vec<UploadObject>> = Vec::new();
            let mut current_batch: Vec<UploadObject> = Vec::new();
            let mut inline_bytes: usize = 0;

            for (object_id, type_tag, cof_data) in &pending {
                let size = cof_data.len();

                if inline_bytes + size > INLINE_BATCH_MAX_BYTES || current_batch.len() >= 500 {
                    batches.push(std::mem::take(&mut current_batch));
                    inline_bytes = 0;
                }

                current_batch.push(UploadObject {
                    object_id: object_id.clone(),
                    type_tag: *type_tag,
                    size_bytes: size,
                    cof_base64: Some(BASE64_STANDARD.encode(cof_data)),
                });
                inline_bytes += size;
            }
            if !current_batch.is_empty() {
                batches.push(current_batch);
            }

            // Send all batches concurrently with bounded parallelism.
            let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS));
            let mut join_set = tokio::task::JoinSet::new();

            for batch in batches {
                let client = self.clone();
                let url = url.clone();
                let map = prepared_map.clone();
                let sem = semaphore.clone();

                join_set.spawn(async move {
                    let _permit = sem.acquire().await.map_err(|_| {
                        SyncError::TransferFailed("semaphore closed".to_string())
                    })?;
                    client.send_upload_batch(&url, batch, &*map).await
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok(batch_accepted)) => accepted_ids.extend(batch_accepted),
                    Ok(Err(sync_err)) => return Err(sync_err),
                    Err(join_err) => {
                        return Err(SyncError::TransferFailed(format!(
                            "batch upload task panicked: {join_err}"
                        )));
                    }
                }
            }

            // Check if all objects are accepted.
            if all_ids.iter().all(|id| accepted_ids.contains(id)) {
                break;
            }

            // Re-queue unaccepted objects for retry (preserving topological order).
            if round < MAX_RETRIES {
                pending = pending
                    .into_iter()
                    .filter(|(hex, _, _)| {
                        ObjectId::from_hex(hex)
                            .map(|id| !accepted_ids.contains(&id))
                            .unwrap_or(false)
                    })
                    .collect();

                if !pending.is_empty() {
                    tracing::debug!(
                        "retry round {}: {} objects not yet accepted",
                        round + 1,
                        pending.len()
                    );
                }
            }
        }

        let accepted: Vec<_> = accepted_ids
            .into_iter()
            .map(|id| proto::common::ObjectId {
                hash: id.as_bytes().to_vec(),
            })
            .collect();

        Ok(PushObjectsResponse {
            success: true,
            message: format!("accepted {} objects", accepted.len()),
            accepted,
        })
    }
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    #[serde(rename = "serverVersion")]
    server_version: Option<String>,
    capabilities: Option<Vec<String>>,
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
    #[serde(rename = "sizeBytes")]
    size_bytes: usize,
    #[serde(rename = "cofBase64", skip_serializing_if = "Option::is_none")]
    cof_base64: Option<String>,
}

#[derive(Debug, Serialize)]
struct UploadRequest {
    objects: Vec<UploadObject>,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    accepted: Vec<String>,
    #[serde(rename = "requiredUploads", default)]
    required_uploads: Vec<RequiredUpload>,
}

#[derive(Debug, Deserialize)]
struct RequiredUpload {
    #[serde(rename = "objectId")]
    object_id: String,
    #[serde(rename = "uploadId")]
    upload_id: String,
    #[serde(rename = "chunkSize")]
    chunk_size: usize,
    #[serde(rename = "totalChunks")]
    total_chunks: usize,
}

#[derive(Debug, Serialize)]
struct DownloadRequest {
    want: Vec<String>,
    have: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct DownloadEnvelope {
    objects: Vec<DownloadManifest>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DownloadManifest {
    #[serde(rename = "objectId")]
    object_id: String,
    #[serde(rename = "typeTag")]
    type_tag: i32,
    #[serde(rename = "sizeBytes")]
    size_bytes: usize,
}

#[derive(Debug, Deserialize)]
struct LegacyDownloadEnvelope {
    objects: Vec<LegacyDownloadObject>,
}

#[derive(Debug, Deserialize)]
struct LegacyDownloadObject {
    #[serde(rename = "objectId")]
    object_id: String,
    #[serde(rename = "typeTag")]
    type_tag: i32,
    #[serde(rename = "cofBase64")]
    cof_base64: String,
}

#[async_trait]
impl SyncTransport for HttpSyncClient {
    async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
        self.ensure_health().await?;
        let mut caps: Vec<String> = self.server_capabilities.iter().cloned().collect();
        caps.sort();
        Ok(HelloResponse {
            server_version: self
                .server_version
                .clone()
                .unwrap_or_else(|| "clawlab-http-v1".to_string()),
            capabilities: caps,
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
        self.ensure_health().await?;

        // Prefer capability negotiation; if the server doesn't advertise capabilities yet, try
        // chunked first (newer servers) and fall back to legacy.
        if self.capabilities_advertised
            && !self.server_capabilities.contains(CAP_CHUNKED_OBJECTS_V1)
        {
            return self.fetch_objects_legacy(store, want, have).await;
        }

        match self.fetch_objects_chunked(store, want, have).await {
            Ok(result) => Ok(result),
            Err(err) => {
                if self.capabilities_advertised {
                    return Err(err);
                }
                self.fetch_objects_legacy(store, want, have).await
            }
        }
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
        self.ensure_health().await?;

        if self.capabilities_advertised
            && !self.server_capabilities.contains(CAP_CHUNKED_OBJECTS_V1)
        {
            return self.push_objects_legacy(store, ids).await;
        }

        match self.push_objects_chunked(store, ids).await {
            Ok(result) => Ok(result),
            Err(err) => {
                if self.capabilities_advertised {
                    return Err(err);
                }
                self.push_objects_legacy(store, ids).await
            }
        }
    }
}
