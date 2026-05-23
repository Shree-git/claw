use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use base64::prelude::*;
use claw_core::cof::{cof_decode, cof_peek_type_tag};
use claw_core::id::ObjectId;
use claw_core::object::{Object, TypeTag};
use claw_store::ClawStore;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::proto;
use crate::proto::sync::{
    HelloResponse, PartialCloneFilter, PushObjectsResponse, UpdateRefsResponse,
};
use crate::protocol::{CAP_HOSTED_HTTP, CAP_PARTIAL_CLONE, CAP_POLICY_AWARE_PUSH, CAP_PROTOCOL_V1};
use crate::security::{redacted_secret_marker, REPLAY_NONCE_METADATA_KEY};
use crate::transport::{RefUpdateContext, SyncTransport};
use crate::SyncError;

#[derive(Clone)]
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

impl std::fmt::Debug for HttpSyncClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut caps: Vec<_> = self.server_capabilities.iter().collect();
        caps.sort();
        f.debug_struct("HttpSyncClient")
            .field("base_url", &self.base_url)
            .field("repo", &self.repo)
            .field(
                "bearer_token",
                &redacted_secret_marker(self.bearer_token.is_some()),
            )
            .field("health_checked", &self.health_checked)
            .field("server_version", &self.server_version)
            .field("server_capabilities", &caps)
            .field("capabilities_advertised", &self.capabilities_advertised)
            .finish()
    }
}

// Keep transfers under Vercel's hard request/response size limits.
const OBJECT_BYTES_CHUNK_SIZE: usize = 4_000_000;
const INLINE_OBJECT_MAX_BYTES: usize = 1_000_000;
const INLINE_BATCH_MAX_BYTES: usize = 2_500_000;
const CAP_CHUNKED_OBJECTS: &str = "chunked-objects";
const CAP_PACK_UPLOAD: &str = "pack-upload";
const CAP_BATCH_COMPLETE: &str = "batch-complete";
const MAX_CONCURRENT_UPLOADS: usize = 8;
const MAX_BATCH_SIZE: usize = 500;

fn legacy_http_fallback_capabilities() -> HashSet<String> {
    [
        CAP_PROTOCOL_V1.to_string(),
        CAP_PARTIAL_CLONE.to_string(),
        "polling-events".to_string(),
    ]
    .into_iter()
    .collect()
}

fn validate_hosted_capability_contract(
    capabilities_advertised: bool,
    capabilities: &HashSet<String>,
) -> Result<(), SyncError> {
    if !capabilities_advertised {
        return Ok(());
    }

    for required in [CAP_PROTOCOL_V1, CAP_HOSTED_HTTP] {
        if !capabilities.contains(required) {
            return Err(SyncError::NegotiationFailed(format!(
                "clawlab HTTP remote advertised capabilities but does not include {required} capability"
            )));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Prepared object: raw COF bytes read directly from disk (no decode/re-encode)
// ---------------------------------------------------------------------------

struct PreparedObject {
    id: ObjectId,
    hex: String,
    type_tag: i32,
    cof_bytes: Vec<u8>,
}

/// Read raw COF bytes from the store without the decode → re-encode cycle.
#[allow(clippy::result_large_err)]
fn prepare_objects_raw(
    store: &ClawStore,
    ids: &[ObjectId],
) -> Result<Vec<PreparedObject>, SyncError> {
    let mut prepared = Vec::with_capacity(ids.len());
    for id in ids {
        let cof_bytes = store.load_cof_bytes(id)?;
        let type_tag = cof_peek_type_tag(&cof_bytes)?;
        prepared.push(PreparedObject {
            id: *id,
            hex: id.to_hex(),
            type_tag: type_tag as i32,
            cof_bytes,
        });
    }
    Ok(prepared)
}

/// Build a CLPK packfile from prepared objects.
///
/// Format: [4B "CLPK"][4B version=1][4B object_count][entries: 4B length, COF bytes]*
fn build_clpk_pack(objects: &[PreparedObject]) -> Vec<u8> {
    let total_cof: usize = objects.iter().map(|o| o.cof_bytes.len()).sum();
    let mut data = Vec::with_capacity(12 + objects.len() * 4 + total_cof);

    data.extend_from_slice(b"CLPK");
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&(objects.len() as u32).to_le_bytes());

    for obj in objects {
        let len = obj.cof_bytes.len() as u32;
        data.extend_from_slice(&len.to_le_bytes());
        data.extend_from_slice(&obj.cof_bytes);
    }

    data
}

fn ids_to_proto(ids: impl IntoIterator<Item = ObjectId>) -> Vec<proto::common::ObjectId> {
    ids.into_iter()
        .map(|id| proto::common::ObjectId {
            hash: id.as_bytes().to_vec(),
        })
        .collect()
}

#[allow(clippy::result_large_err)]
fn parse_hex_ids(hexes: &[String]) -> Result<Vec<ObjectId>, SyncError> {
    hexes
        .iter()
        .map(|hex| {
            ObjectId::from_hex(hex)
                .map_err(|e| SyncError::TransferFailed(format!("invalid object id: {e}")))
        })
        .collect()
}

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
        format!("{}/sync/repos/{}{}", self.base_url, repo, suffix)
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
            builder = builder
                .header("idempotency-key", key.clone())
                .header(REPLAY_NONCE_METADATA_KEY, key);
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
        self.server_version = Some(
            health
                .server_version
                .unwrap_or_else(|| "clawlab-http".to_string()),
        );
        self.capabilities_advertised = health.capabilities.is_some();
        self.server_capabilities = health
            .capabilities
            .unwrap_or_default()
            .into_iter()
            .collect();

        // Older servers may not advertise capabilities; assume a minimal baseline.
        if self.server_capabilities.is_empty() && !self.capabilities_advertised {
            self.server_capabilities = legacy_http_fallback_capabilities();
        }
        validate_hosted_capability_contract(
            self.capabilities_advertised,
            &self.server_capabilities,
        )?;

        self.health_checked = true;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Fetch helpers
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Upload helpers (individual object chunked upload)
    // -----------------------------------------------------------------------

    /// Upload object data chunks and complete the upload session.
    async fn upload_object_chunks(
        &self,
        object_id: &str,
        upload_id: &str,
        cof_bytes: &[u8],
        chunk_size: usize,
        total_chunks: usize,
    ) -> Result<(), SyncError> {
        self.upload_object_data_chunks(object_id, upload_id, cof_bytes, chunk_size, total_chunks)
            .await?;
        self.complete_single_upload(object_id, upload_id).await
    }

    /// Upload data chunks without completing the upload session.
    /// Used with batch-complete (Tier 3) to amortise completion overhead.
    async fn upload_object_data_chunks(
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
                    object_id, idx, status, body
                )));
            }
        }

        Ok(())
    }

    /// Complete a single upload session.
    async fn complete_single_upload(
        &self,
        object_id: &str,
        upload_id: &str,
    ) -> Result<(), SyncError> {
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
                object_id, status, body
            )));
        }
        Ok(())
    }

    /// Complete multiple upload sessions in a single request (Tier 3).
    async fn batch_complete_uploads(
        &self,
        entries: &[(String, String)],
    ) -> Result<Vec<ObjectId>, SyncError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let url = self.endpoint("/objects:batch-complete");
        let payload = BatchCompleteRequest {
            uploads: entries
                .iter()
                .map(|(oid, uid)| BatchCompleteEntry {
                    object_id: oid.clone(),
                    upload_id: uid.clone(),
                })
                .collect(),
        };

        let resp = self
            .request(reqwest::Method::POST, url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::TransferFailed(format!(
                "batch complete failed: {} body={}",
                status, body
            )));
        }

        let body: BatchCompleteResponse = resp.json().await?;
        parse_hex_ids(&body.accepted)
    }

    // -----------------------------------------------------------------------
    // Batch upload + required object uploads
    // -----------------------------------------------------------------------

    async fn send_upload_batch(
        &self,
        url: &str,
        batch: Vec<UploadObject>,
        prepared_map: &HashMap<String, Vec<u8>>,
        use_batch_complete: bool,
    ) -> Result<(HashSet<ObjectId>, Vec<(String, String)>), SyncError> {
        if batch.is_empty() {
            return Ok((HashSet::new(), Vec::new()));
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
                status, body
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
        let (completed_uploads, pending_completes) = self
            .upload_required_objects(body.required_uploads, prepared_map, use_batch_complete)
            .await?;
        // With batch-complete support, these uploads are only staged and are not
        // accepted until /objects:batch-complete returns them as accepted.
        if use_batch_complete {
            debug_assert!(completed_uploads.is_empty());
        } else {
            accepted_ids.extend(completed_uploads);
        }

        Ok((accepted_ids, pending_completes))
    }

    async fn upload_required_objects(
        &self,
        required_uploads: Vec<RequiredUpload>,
        prepared_map: &HashMap<String, Vec<u8>>,
        use_batch_complete: bool,
    ) -> Result<(Vec<ObjectId>, Vec<(String, String)>), SyncError> {
        if required_uploads.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Collect entries for batch-complete if supported.
        let pending_completes: Vec<(String, String)> = if use_batch_complete {
            required_uploads
                .iter()
                .map(|r| (r.object_id.clone(), r.upload_id.clone()))
                .collect()
        } else {
            Vec::new()
        };

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
            let skip_complete = use_batch_complete;

            join_set.spawn(async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| SyncError::TransferFailed("semaphore closed".to_string()))?;

                if skip_complete {
                    // Tier 3: upload data only; completion deferred to batch.
                    client
                        .upload_object_data_chunks(
                            &required.object_id,
                            &required.upload_id,
                            &cof,
                            required.chunk_size,
                            required.total_chunks,
                        )
                        .await?;
                    Ok::<Option<ObjectId>, SyncError>(None)
                } else {
                    client
                        .upload_object_chunks(
                            &required.object_id,
                            &required.upload_id,
                            &cof,
                            required.chunk_size,
                            required.total_chunks,
                        )
                        .await?;
                    let id = ObjectId::from_hex(&required.object_id).map_err(|e| {
                        SyncError::TransferFailed(format!("invalid object id: {e}"))
                    })?;
                    Ok(Some(id))
                }
            });
        }

        let mut uploaded_ids = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(Some(id))) => uploaded_ids.push(id),
                Ok(Ok(None)) => {}
                Ok(Err(sync_err)) => return Err(sync_err),
                Err(join_err) => {
                    return Err(SyncError::TransferFailed(format!(
                        "upload task panicked: {join_err}"
                    )));
                }
            }
        }

        Ok((uploaded_ids, pending_completes))
    }

    // -----------------------------------------------------------------------
    // Tier 1: Pack upload – single binary CLPK payload
    // -----------------------------------------------------------------------

    async fn push_objects_pack(
        &self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let prepared = prepare_objects_raw(store, ids)?;
        let pack_data = build_clpk_pack(&prepared);
        let pack_size = pack_data.len();

        let accepted = if pack_size <= OBJECT_BYTES_CHUNK_SIZE {
            self.push_pack_inline(pack_data).await?
        } else {
            self.push_pack_chunked(pack_data).await?
        };

        Ok(PushObjectsResponse {
            success: true,
            message: format!("accepted {} objects (pack)", accepted.len()),
            accepted: ids_to_proto(accepted),
        })
    }

    /// Small pack (≤ 4 MB): single POST with binary CLPK body.
    async fn push_pack_inline(&self, pack_data: Vec<u8>) -> Result<Vec<ObjectId>, SyncError> {
        let url = self.endpoint("/objects:pack-upload");
        let resp = self
            .request(reqwest::Method::POST, url)
            .header(reqwest::header::CONTENT_TYPE, "application/x-clpk")
            .body(pack_data)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::TransferFailed(format!(
                "pack upload failed: {} body={}",
                status, body
            )));
        }

        let body: PackUploadResponse = resp.json().await?;
        parse_hex_ids(&body.accepted)
    }

    /// Large pack (> 4 MB): initiate a pack upload session and stream chunks.
    async fn push_pack_chunked(&self, pack_data: Vec<u8>) -> Result<Vec<ObjectId>, SyncError> {
        let pack_size = pack_data.len();

        // 1. Initiate pack upload session.
        let url = self.endpoint("/objects:pack-upload");
        let payload = PackUploadInitRequest {
            pack_size_bytes: pack_size,
        };
        let resp = self
            .request(reqwest::Method::POST, url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::TransferFailed(format!(
                "pack upload init failed: {} body={}",
                status, body
            )));
        }

        let init: PackUploadInitResponse = resp.json().await?;
        if init.chunk_size == 0 {
            return Err(SyncError::TransferFailed(
                "pack upload init returned invalid chunkSize=0".to_string(),
            ));
        }
        let expected_total_chunks =
            pack_size.checked_add(init.chunk_size - 1).ok_or_else(|| {
                SyncError::TransferFailed(format!(
                    "invalid pack upload chunk plan: overflow for pack_size={} chunk_size={}",
                    pack_size, init.chunk_size
                ))
            })? / init.chunk_size;
        if init.total_chunks != expected_total_chunks {
            return Err(SyncError::TransferFailed(format!(
                "invalid pack upload chunk plan: totalChunks={} expected={} \
                 (packSize={}, chunkSize={})",
                init.total_chunks, expected_total_chunks, pack_size, init.chunk_size
            )));
        }

        // 2. Upload chunks concurrently.
        let pack_data = Arc::new(pack_data);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS));
        let mut join_set = tokio::task::JoinSet::new();

        for idx in 0..init.total_chunks {
            let start = idx.checked_mul(init.chunk_size).ok_or_else(|| {
                SyncError::TransferFailed(format!(
                    "invalid pack upload chunk plan: overflow at idx={} chunkSize={}",
                    idx, init.chunk_size
                ))
            })?;
            if start >= pack_size {
                return Err(SyncError::TransferFailed(format!(
                    "invalid pack upload chunk plan: chunk idx {} starts at {} beyond pack size {}",
                    idx, start, pack_size
                )));
            }
            let end = std::cmp::min(start.saturating_add(init.chunk_size), pack_size);
            if end <= start {
                return Err(SyncError::TransferFailed(format!(
                    "invalid pack upload chunk plan: empty chunk range for idx {}",
                    idx
                )));
            }
            let chunk = pack_data[start..end].to_vec();

            let client = self.clone();
            let upload_id = init.upload_id.clone();
            let sem = semaphore.clone();

            join_set.spawn(async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| SyncError::TransferFailed("semaphore closed".to_string()))?;

                let url = client.endpoint(&format!(
                    "/objects/pack-uploads/{}/chunks/{}",
                    urlencoding::encode(&upload_id),
                    idx
                ));
                let resp = client
                    .request(reqwest::Method::PUT, url)
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(chunk)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(SyncError::TransferFailed(format!(
                        "pack chunk upload failed idx {}: {} body={}",
                        idx, status, body
                    )));
                }

                Ok::<(), SyncError>(())
            });
        }

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(sync_err)) => return Err(sync_err),
                Err(join_err) => {
                    return Err(SyncError::TransferFailed(format!(
                        "pack chunk upload task panicked: {join_err}"
                    )));
                }
            }
        }

        // 3. Complete pack upload.
        let url = self.endpoint(&format!(
            "/objects/pack-uploads/{}:complete",
            urlencoding::encode(&init.upload_id)
        ));
        let resp = self.request(reqwest::Method::POST, url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SyncError::TransferFailed(format!(
                "pack upload complete failed: {} body={}",
                status, body
            )));
        }

        let body: PackUploadResponse = resp.json().await?;
        parse_hex_ids(&body.accepted)
    }

    // -----------------------------------------------------------------------
    // Tier 2: Hybrid inline + chunked push (replaces push_objects_chunked)
    //
    // Small objects (≤ 1 MB) are inlined in the batch POST via cofBase64,
    // eliminating 2 HTTP round-trips per object (chunk PUT + complete POST).
    // Large objects still use chunked upload sessions.
    // A retry loop handles dependency ordering (same as legacy path).
    // -----------------------------------------------------------------------

    async fn push_objects_hybrid(
        &self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let prepared = prepare_objects_raw(store, ids)?;

        let prepared_map: Arc<HashMap<String, Vec<u8>>> = Arc::new(
            prepared
                .iter()
                .map(|p| (p.hex.clone(), p.cof_bytes.clone()))
                .collect(),
        );

        let url = self.endpoint("/objects:batch-upload");
        let all_ids: HashSet<ObjectId> = ids.iter().copied().collect();
        let mut accepted_ids: HashSet<ObjectId> = HashSet::new();
        let use_batch_complete = self.server_capabilities.contains(CAP_BATCH_COMPLETE);

        // Retry loop for dependency ordering: inline objects whose parents
        // haven't been stored yet will be rejected and retried.
        let mut pending = prepared;
        const MAX_RETRIES: usize = 3;

        for round in 0..=MAX_RETRIES {
            if pending.is_empty() {
                break;
            }

            // Build batches, inlining objects that fit.
            let mut batches: Vec<Vec<UploadObject>> = Vec::new();
            let mut current_batch: Vec<UploadObject> = Vec::new();
            let mut batch_inline_bytes: usize = 0;

            for obj in &pending {
                let is_inline = obj.cof_bytes.len() <= INLINE_OBJECT_MAX_BYTES;
                let inline_size = if is_inline { obj.cof_bytes.len() } else { 0 };

                // Flush batch if limits exceeded.
                if current_batch.len() >= MAX_BATCH_SIZE
                    || (is_inline
                        && batch_inline_bytes + inline_size > INLINE_BATCH_MAX_BYTES
                        && !current_batch.is_empty())
                {
                    batches.push(std::mem::take(&mut current_batch));
                    batch_inline_bytes = 0;
                }

                current_batch.push(UploadObject {
                    object_id: obj.hex.clone(),
                    type_tag: obj.type_tag,
                    size_bytes: obj.cof_bytes.len(),
                    cof_base64: if is_inline {
                        Some(BASE64_STANDARD.encode(&obj.cof_bytes))
                    } else {
                        None
                    },
                });

                if is_inline {
                    batch_inline_bytes += inline_size;
                }
            }
            if !current_batch.is_empty() {
                batches.push(current_batch);
            }

            // Send all batches concurrently with bounded parallelism.
            let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS));
            let mut join_set = tokio::task::JoinSet::new();
            let mut all_pending_completes: Vec<(String, String)> = Vec::new();

            for batch in batches {
                let client = self.clone();
                let url = url.clone();
                let map = prepared_map.clone();
                let sem = semaphore.clone();
                let batch_complete = use_batch_complete;

                join_set.spawn(async move {
                    let _permit = sem
                        .acquire()
                        .await
                        .map_err(|_| SyncError::TransferFailed("semaphore closed".to_string()))?;
                    client
                        .send_upload_batch(&url, batch, &map, batch_complete)
                        .await
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok((batch_accepted, pending_completes))) => {
                        accepted_ids.extend(batch_accepted);
                        all_pending_completes.extend(pending_completes);
                    }
                    Ok(Err(sync_err)) => return Err(sync_err),
                    Err(join_err) => {
                        return Err(SyncError::TransferFailed(format!(
                            "batch upload task panicked: {join_err}"
                        )));
                    }
                }
            }

            // Tier 3: batch-complete all pending uploads in one request.
            if !all_pending_completes.is_empty() {
                let batch_accepted = self.batch_complete_uploads(&all_pending_completes).await?;
                accepted_ids.extend(batch_accepted);
            }

            // Check if all objects are accepted.
            if all_ids.iter().all(|id| accepted_ids.contains(id)) {
                break;
            }

            // Re-queue unaccepted objects for retry (preserving topological order).
            if round < MAX_RETRIES {
                pending.retain(|obj| !accepted_ids.contains(&obj.id));

                if !pending.is_empty() {
                    tracing::debug!(
                        "retry round {}: {} objects not yet accepted",
                        round + 1,
                        pending.len()
                    );
                }
            }
        }

        Ok(PushObjectsResponse {
            success: true,
            message: format!("accepted {} objects", accepted_ids.len()),
            accepted: ids_to_proto(accepted_ids),
        })
    }

    // -----------------------------------------------------------------------
    // Fetch (chunked + legacy)
    // -----------------------------------------------------------------------

    async fn fetch_objects_chunked(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
        filter: Option<&PartialCloneFilter>,
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
                filter: filter.map(HttpPartialCloneFilter::from),
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

    async fn fetch_objects_legacy(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
        filter: Option<&PartialCloneFilter>,
    ) -> Result<Vec<ObjectId>, SyncError> {
        let url = self.endpoint("/objects:batch-download");
        let payload = DownloadRequest {
            want: want.iter().map(ObjectId::to_hex).collect(),
            have: have.iter().map(ObjectId::to_hex).collect(),
            cursor: None,
            limit: None,
            filter: filter.map(HttpPartialCloneFilter::from),
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

            let cof_bytes = BASE64_STANDARD.decode(item.cof_base64).map_err(|e| {
                SyncError::TransferFailed(format!("invalid cofBase64 for {object_id}: {e}"))
            })?;

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

    // -----------------------------------------------------------------------
    // Legacy push (inline-only, for servers without chunked-objects)
    // -----------------------------------------------------------------------

    async fn push_objects_legacy(
        &self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        let prepared = prepare_objects_raw(store, ids)?;

        let prepared_map: Arc<HashMap<String, Vec<u8>>> = Arc::new(
            prepared
                .iter()
                .map(|p| (p.hex.clone(), p.cof_bytes.clone()))
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

            for obj in &pending {
                if obj.cof_bytes.len() > INLINE_OBJECT_MAX_BYTES {
                    return Err(SyncError::TransferFailed(format!(
                        "server does not advertise {CAP_CHUNKED_OBJECTS}; \
                         cannot push large object {} ({} bytes)",
                        obj.hex,
                        obj.cof_bytes.len()
                    )));
                }
            }

            // Build batches from pending objects.
            let mut batches: Vec<Vec<UploadObject>> = Vec::new();
            let mut current_batch: Vec<UploadObject> = Vec::new();
            let mut inline_bytes: usize = 0;

            for obj in &pending {
                let size = obj.cof_bytes.len();

                if inline_bytes + size > INLINE_BATCH_MAX_BYTES
                    || current_batch.len() >= MAX_BATCH_SIZE
                {
                    batches.push(std::mem::take(&mut current_batch));
                    inline_bytes = 0;
                }

                current_batch.push(UploadObject {
                    object_id: obj.hex.clone(),
                    type_tag: obj.type_tag,
                    size_bytes: size,
                    cof_base64: Some(BASE64_STANDARD.encode(&obj.cof_bytes)),
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
                    let _permit = sem
                        .acquire()
                        .await
                        .map_err(|_| SyncError::TransferFailed("semaphore closed".to_string()))?;
                    client.send_upload_batch(&url, batch, &map, false).await
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok((batch_accepted, _))) => accepted_ids.extend(batch_accepted),
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
                pending.retain(|obj| !accepted_ids.contains(&obj.id));

                if !pending.is_empty() {
                    tracing::debug!(
                        "retry round {}: {} objects not yet accepted",
                        round + 1,
                        pending.len()
                    );
                }
            }
        }

        Ok(PushObjectsResponse {
            success: true,
            message: format!("accepted {} objects", accepted_ids.len()),
            accepted: ids_to_proto(accepted_ids),
        })
    }
}

// ---------------------------------------------------------------------------
// Serde types
// ---------------------------------------------------------------------------

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
    #[serde(rename = "policyReceipt", skip_serializing_if = "Option::is_none")]
    policy_receipt: Option<RefUpdateContext>,
}

#[derive(Debug, Deserialize)]
struct CasUpdateResponse {
    success: bool,
    message: String,
}

// Batch upload (existing)

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

// Pack upload (Tier 1)

#[derive(Debug, Serialize)]
struct PackUploadInitRequest {
    #[serde(rename = "packSizeBytes")]
    pack_size_bytes: usize,
}

#[derive(Debug, Deserialize)]
struct PackUploadInitResponse {
    #[serde(rename = "uploadId")]
    upload_id: String,
    #[serde(rename = "chunkSize")]
    chunk_size: usize,
    #[serde(rename = "totalChunks")]
    total_chunks: usize,
}

#[derive(Debug, Deserialize)]
struct PackUploadResponse {
    accepted: Vec<String>,
}

// Batch complete (Tier 3)

#[derive(Debug, Serialize)]
struct BatchCompleteRequest {
    uploads: Vec<BatchCompleteEntry>,
}

#[derive(Debug, Serialize)]
struct BatchCompleteEntry {
    #[serde(rename = "objectId")]
    object_id: String,
    #[serde(rename = "uploadId")]
    upload_id: String,
}

#[derive(Debug, Deserialize)]
struct BatchCompleteResponse {
    accepted: Vec<String>,
}

// Download

#[derive(Debug, Serialize)]
struct DownloadRequest {
    want: Vec<String>,
    have: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<HttpPartialCloneFilter>,
}

#[derive(Debug, Serialize)]
struct HttpPartialCloneFilter {
    #[serde(rename = "intentIds", skip_serializing_if = "Vec::is_empty")]
    intent_ids: Vec<String>,
    #[serde(rename = "pathPrefixes", skip_serializing_if = "Vec::is_empty")]
    path_prefixes: Vec<String>,
    #[serde(rename = "timeRangeStart", skip_serializing_if = "Option::is_none")]
    time_range_start: Option<u64>,
    #[serde(rename = "timeRangeEnd", skip_serializing_if = "Option::is_none")]
    time_range_end: Option<u64>,
    #[serde(rename = "codecIds", skip_serializing_if = "Vec::is_empty")]
    codec_ids: Vec<String>,
    #[serde(rename = "capsuleVisibility", skip_serializing_if = "Option::is_none")]
    capsule_visibility: Option<String>,
    #[serde(rename = "maxBytes", skip_serializing_if = "Option::is_none")]
    max_bytes: Option<u64>,
    #[serde(rename = "maxDepth", skip_serializing_if = "Option::is_none")]
    max_depth: Option<u32>,
}

impl From<&PartialCloneFilter> for HttpPartialCloneFilter {
    fn from(filter: &PartialCloneFilter) -> Self {
        Self {
            intent_ids: filter.intent_ids.clone(),
            path_prefixes: filter.path_prefixes.clone(),
            time_range_start: (filter.time_range_start > 0).then_some(filter.time_range_start),
            time_range_end: (filter.time_range_end > 0).then_some(filter.time_range_end),
            codec_ids: filter.codec_ids.clone(),
            capsule_visibility: (!filter.capsule_visibility.trim().is_empty())
                .then(|| filter.capsule_visibility.clone()),
            max_bytes: (filter.max_bytes > 0).then_some(filter.max_bytes),
            max_depth: (filter.max_depth > 0).then_some(filter.max_depth),
        }
    }
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

// ---------------------------------------------------------------------------
// SyncTransport impl
// ---------------------------------------------------------------------------

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
                .unwrap_or_else(|| "clawlab-http".to_string()),
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
        filter: Option<PartialCloneFilter>,
    ) -> Result<Vec<ObjectId>, SyncError> {
        self.ensure_health().await?;

        if filter.is_some()
            && self.capabilities_advertised
            && !self.server_capabilities.contains(CAP_PARTIAL_CLONE)
        {
            return Err(SyncError::TransferFailed(
                "clawlab HTTP remote does not advertise partial-clone capability".to_string(),
            ));
        }

        // Prefer capability negotiation; if the server doesn't advertise capabilities yet, try
        // chunked first (newer servers) and fall back to legacy.
        if self.capabilities_advertised && !self.server_capabilities.contains(CAP_CHUNKED_OBJECTS) {
            return self
                .fetch_objects_legacy(store, want, have, filter.as_ref())
                .await;
        }

        match self
            .fetch_objects_chunked(store, want, have, filter.as_ref())
            .await
        {
            Ok(result) => Ok(result),
            Err(err) => {
                if self.capabilities_advertised {
                    return Err(err);
                }
                self.fetch_objects_legacy(store, want, have, filter.as_ref())
                    .await
            }
        }
    }

    async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError> {
        self.update_refs_with_context(updates, force, None).await
    }

    async fn update_refs_with_context(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
        context: Option<RefUpdateContext>,
    ) -> Result<UpdateRefsResponse, SyncError> {
        self.ensure_health().await?;
        if context
            .as_ref()
            .is_some_and(RefUpdateContext::has_policy_checks)
            && !self.server_capabilities.contains(CAP_POLICY_AWARE_PUSH)
        {
            return Err(SyncError::TransferFailed(format!(
                "clawlab HTTP remote does not advertise {CAP_POLICY_AWARE_PUSH} capability"
            )));
        }

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
            policy_receipt: context,
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

        // Strategy 1: Pack upload (Tier 1) – single binary payload, fewest HTTP
        // requests. The server unpacks the CLPK and stores all objects at once.
        if self.server_capabilities.contains(CAP_PACK_UPLOAD) {
            return self.push_objects_pack(store, ids).await;
        }

        // Strategy 2: Hybrid inline + chunked (Tier 2) – inline small objects
        // to avoid 2 extra HTTP round-trips per object, with chunked upload
        // for large objects and optional batch-complete (Tier 3).
        if self.capabilities_advertised && self.server_capabilities.contains(CAP_CHUNKED_OBJECTS) {
            return self.push_objects_hybrid(store, ids).await;
        }

        // Strategy 3: Legacy inline-only for old servers.
        if self.capabilities_advertised {
            return self.push_objects_legacy(store, ids).await;
        }

        // Unknown capabilities: try hybrid, fall back to legacy.
        match self.push_objects_hybrid(store, ids).await {
            Ok(result) => Ok(result),
            Err(_) => self.push_objects_legacy(store, ids).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_bearer_token() {
        let client = HttpSyncClient::new(
            "https://sync.example.test".to_string(),
            "repo".to_string(),
            Some("super-secret-token".to_string()),
        );

        let rendered = format!("{client:?}");
        assert!(!rendered.contains("super-secret-token"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn legacy_health_fallback_capabilities_include_protocol_marker() {
        let fallback = legacy_http_fallback_capabilities();
        let caps: std::collections::HashSet<_> = fallback.iter().map(String::as_str).collect();
        assert!(caps.contains(CAP_PROTOCOL_V1));
        assert!(caps.contains(CAP_PARTIAL_CLONE));
        assert!(caps.contains("polling-events"));
        assert!(
            !caps.contains(CAP_HOSTED_HTTP),
            "legacy fallback is not the advertised hosted HTTP contract"
        );
    }

    #[test]
    fn advertised_hosted_capabilities_require_protocol_and_endpoint_marker() {
        validate_hosted_capability_contract(false, &HashSet::new())
            .expect("legacy remotes without capabilities remain compatible");

        let err = validate_hosted_capability_contract(
            true,
            &[CAP_HOSTED_HTTP.to_string()].into_iter().collect(),
        )
        .expect_err("advertised hosted capabilities must include protocol marker");
        assert!(
            err.to_string().contains(CAP_PROTOCOL_V1),
            "unexpected error: {err}"
        );

        let err = validate_hosted_capability_contract(
            true,
            &[CAP_PROTOCOL_V1.to_string()].into_iter().collect(),
        )
        .expect_err("advertised hosted capabilities must include hosted-http marker");
        assert!(
            err.to_string().contains(CAP_HOSTED_HTTP),
            "unexpected error: {err}"
        );

        let complete = [CAP_PROTOCOL_V1.to_string(), CAP_HOSTED_HTTP.to_string()]
            .into_iter()
            .collect::<HashSet<_>>();
        validate_hosted_capability_contract(true, &complete)
            .expect("complete hosted capability baseline");
    }

    #[test]
    fn download_request_serializes_partial_clone_filter_shape() {
        let request = DownloadRequest {
            want: vec!["a".to_string()],
            have: vec!["b".to_string()],
            cursor: None,
            limit: Some(2000),
            filter: Some(HttpPartialCloneFilter::from(&PartialCloneFilter {
                intent_ids: vec!["01H00000000000000000000000".to_string()],
                path_prefixes: vec!["src/".to_string()],
                time_range_start: 1000,
                time_range_end: 2000,
                codec_ids: vec!["json/tree".to_string()],
                capsule_visibility: "public".to_string(),
                max_bytes: 4096,
                max_depth: 3,
            })),
        };

        let value = serde_json::to_value(&request).expect("serialize download request");
        assert_eq!(
            value["filter"]["intentIds"][0],
            "01H00000000000000000000000"
        );
        assert_eq!(value["filter"]["pathPrefixes"][0], "src/");
        assert_eq!(value["filter"]["timeRangeStart"], 1000);
        assert_eq!(value["filter"]["timeRangeEnd"], 2000);
        assert_eq!(value["filter"]["codecIds"][0], "json/tree");
        assert_eq!(value["filter"]["capsuleVisibility"], "public");
        assert_eq!(value["filter"]["maxBytes"], 4096);
        assert_eq!(value["filter"]["maxDepth"], 3);
    }

    #[test]
    fn cas_update_request_serializes_policy_receipt_shape() {
        let request = CasUpdateRequest {
            updates: vec![RefUpdatePayload {
                name: "heads/main".to_string(),
                old_target: Some("old".to_string()),
                new_target: "new".to_string(),
                force: false,
            }],
            policy_receipt: Some(RefUpdateContext {
                policies: vec![crate::transport::RefUpdatePolicyCheck {
                    id: "release-gate".to_string(),
                    ref_name: "refs/policies/release-gate".to_string(),
                    object: "01H00000000000000000000000".to_string(),
                    allowed: true,
                    reason: None,
                }],
                requested_capabilities: vec![CAP_POLICY_AWARE_PUSH.to_string()],
                negotiated_capabilities: vec![CAP_PROTOCOL_V1.to_string()],
            }),
        };

        let value = serde_json::to_value(&request).expect("serialize CAS update request");
        assert_eq!(value["updates"][0]["name"], "heads/main");
        assert_eq!(value["policyReceipt"]["policies"][0]["id"], "release-gate");
        assert_eq!(
            value["policyReceipt"]["policies"][0]["ref"],
            "refs/policies/release-gate"
        );
        assert_eq!(
            value["policyReceipt"]["requestedCapabilities"][0],
            CAP_POLICY_AWARE_PUSH
        );
    }

    #[tokio::test]
    async fn hello_rejects_advertised_hosted_remote_without_endpoint_marker() {
        let base_url = spawn_health_response(
            r#"{"serverVersion":"clawlab-test","capabilities":["protocol:claw-sync/1","partial-clone"]}"#,
        )
        .await;
        let mut client = HttpSyncClient::new(base_url, "repo".to_string(), None);

        let err = client
            .hello()
            .await
            .expect_err("advertised hosted remotes must include hosted-http");
        assert!(
            err.to_string().contains(CAP_HOSTED_HTTP),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn hello_accepts_advertised_hosted_remote_contract_baseline() {
        let base_url = spawn_health_response(
            r#"{"serverVersion":"clawlab-test","capabilities":["protocol:claw-sync/1","hosted-http","partial-clone"]}"#,
        )
        .await;
        let mut client = HttpSyncClient::new(base_url, "repo".to_string(), None);

        let hello = client.hello().await.expect("hosted hello");
        assert_eq!(hello.server_version, "clawlab-test");
        assert!(hello
            .capabilities
            .iter()
            .any(|capability| capability == CAP_HOSTED_HTTP));
    }

    async fn spawn_health_response(body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind health test server");
        let addr = listener.local_addr().expect("health server addr");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept health request");
            let mut request = [0_u8; 1024];
            let _ = stream
                .read(&mut request)
                .await
                .expect("read health request");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write health response");
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn hosted_ref_update_with_policy_receipt_requires_capability() {
        let mut client = HttpSyncClient::new(
            "https://sync.example.test".to_string(),
            "repo".to_string(),
            None,
        );
        client.health_checked = true;
        client.capabilities_advertised = true;
        client.server_version = Some("clawlab-http".to_string());
        client.server_capabilities = [CAP_PROTOCOL_V1.to_string(), CAP_HOSTED_HTTP.to_string()]
            .into_iter()
            .collect();

        let context = RefUpdateContext {
            policies: vec![crate::transport::RefUpdatePolicyCheck {
                id: "release-gate".to_string(),
                ref_name: "refs/policies/release-gate".to_string(),
                object: "01H00000000000000000000000".to_string(),
                allowed: true,
                reason: None,
            }],
            requested_capabilities: vec![CAP_POLICY_AWARE_PUSH.to_string()],
            negotiated_capabilities: vec![CAP_PROTOCOL_V1.to_string()],
        };

        let err = client
            .update_refs_with_context(&[], false, Some(context))
            .await
            .expect_err("policy-aware hosted ref update must fail closed");
        assert!(
            err.to_string().contains(CAP_POLICY_AWARE_PUSH),
            "unexpected error: {err}"
        );
    }
}
