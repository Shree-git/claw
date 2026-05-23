use async_trait::async_trait;
use claw_core::cof::{cof_decode, cof_encode};
use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;
use rand::Rng;
use tokio::time::{sleep, Duration};
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity};
use tonic::Code;

use crate::http_client::HttpSyncClient;
use crate::proto::sync::sync_service_client::SyncServiceClient;
use crate::proto::sync::*;
use crate::protocol::{server_capabilities, SYNC_PROTOCOL_VERSION};
use crate::security::REPLAY_NONCE_METADATA_KEY;
use crate::transport::{GrpcTlsConfig, RefUpdateContext, RemoteTransportConfig, SyncTransport};
use crate::SyncError;

pub struct SyncClient {
    inner: Box<dyn SyncTransport>,
    retry_policy: RetryPolicy,
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub idempotent_only: bool,
    pub max_attempts: u32,
    pub base_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            idempotent_only: true,
            max_attempts: 4,
            base_backoff_ms: 100,
            max_backoff_ms: 2_000,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    fn attempts(self) -> u32 {
        self.max_attempts.max(1)
    }

    fn backoff_delay(self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(31);
        let factor = 1u64 << exponent;
        let base = self.base_backoff_ms.saturating_mul(factor);
        let capped = base.min(self.max_backoff_ms.max(self.base_backoff_ms));

        if !self.jitter {
            return Duration::from_millis(capped);
        }

        let lower = capped / 2;
        let upper = capped.saturating_mul(3).saturating_div(2).max(lower + 1);
        let sampled = rand::thread_rng().gen_range(lower..upper);
        Duration::from_millis(sampled)
    }
}

impl SyncClient {
    pub async fn connect(addr: &str) -> Result<Self, SyncError> {
        Self::connect_with_transport(RemoteTransportConfig::Grpc {
            addr: addr.to_string(),
            bearer_token: None,
            tls: None,
        })
        .await
    }

    pub async fn connect_with_transport(config: RemoteTransportConfig) -> Result<Self, SyncError> {
        Self::connect_with_transport_and_retry(config, RetryPolicy::default()).await
    }

    pub async fn connect_with_transport_and_retry(
        config: RemoteTransportConfig,
        retry_policy: RetryPolicy,
    ) -> Result<Self, SyncError> {
        let inner: Box<dyn SyncTransport> = match config {
            RemoteTransportConfig::Grpc {
                addr,
                bearer_token,
                tls,
            } => Box::new(GrpcSyncClient::connect(&addr, bearer_token, tls).await?),
            RemoteTransportConfig::Http {
                base_url,
                repo,
                bearer_token,
            } => Box::new(HttpSyncClient::new(base_url, repo, bearer_token)),
        };

        Ok(Self {
            inner,
            retry_policy,
        })
    }

    fn should_retry(&self, err: &SyncError) -> bool {
        match err {
            SyncError::Grpc(status) => matches!(
                status.code(),
                Code::Unavailable
                    | Code::DeadlineExceeded
                    | Code::ResourceExhausted
                    | Code::Aborted
                    | Code::Internal
            ),
            SyncError::Transport(_) => true,
            SyncError::Http(err) => {
                if err.is_timeout() || err.is_connect() {
                    return true;
                }

                matches!(
                    err.status(),
                    Some(
                        reqwest::StatusCode::TOO_MANY_REQUESTS
                            | reqwest::StatusCode::BAD_GATEWAY
                            | reqwest::StatusCode::SERVICE_UNAVAILABLE
                            | reqwest::StatusCode::GATEWAY_TIMEOUT
                    )
                )
            }
            _ => false,
        }
    }

    async fn retry_wait(policy: RetryPolicy, attempt: u32) {
        let delay = policy.backoff_delay(attempt);
        sleep(delay).await;
    }

    async fn retry_hello(&mut self) -> Result<HelloResponse, SyncError> {
        let attempts = self.retry_policy.attempts();
        let mut last_err = None;

        for attempt in 1..=attempts {
            match self.inner.hello().await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    let retryable = self.should_retry(&err);
                    last_err = Some(err);
                    if !retryable || attempt == attempts {
                        break;
                    }
                    Self::retry_wait(self.retry_policy, attempt).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            SyncError::ConnectionFailed("retry loop exited without recording an error".to_string())
        }))
    }

    async fn retry_advertise_refs(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, ObjectId)>, SyncError> {
        let attempts = self.retry_policy.attempts();
        let mut last_err = None;

        for attempt in 1..=attempts {
            match self.inner.advertise_refs(prefix).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    let retryable = self.should_retry(&err);
                    last_err = Some(err);
                    if !retryable || attempt == attempts {
                        break;
                    }
                    Self::retry_wait(self.retry_policy, attempt).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            SyncError::ConnectionFailed("retry loop exited without recording an error".to_string())
        }))
    }

    async fn retry_fetch_objects(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
        filter: Option<PartialCloneFilter>,
    ) -> Result<Vec<ObjectId>, SyncError> {
        let attempts = self.retry_policy.attempts();
        let mut last_err = None;

        for attempt in 1..=attempts {
            match self
                .inner
                .fetch_objects(store, want, have, filter.clone())
                .await
            {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    let retryable = self.should_retry(&err);
                    last_err = Some(err);
                    if !retryable || attempt == attempts {
                        break;
                    }
                    Self::retry_wait(self.retry_policy, attempt).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            SyncError::ConnectionFailed("retry loop exited without recording an error".to_string())
        }))
    }

    async fn retry_update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
        context: Option<RefUpdateContext>,
    ) -> Result<UpdateRefsResponse, SyncError> {
        if self.retry_policy.idempotent_only {
            return self
                .inner
                .update_refs_with_context(updates, force, context)
                .await;
        }

        let attempts = self.retry_policy.attempts();
        let mut last_err = None;

        for attempt in 1..=attempts {
            match self
                .inner
                .update_refs_with_context(updates, force, context.clone())
                .await
            {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    let retryable = self.should_retry(&err);
                    last_err = Some(err);
                    if !retryable || attempt == attempts {
                        break;
                    }
                    Self::retry_wait(self.retry_policy, attempt).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            SyncError::ConnectionFailed("retry loop exited without recording an error".to_string())
        }))
    }

    async fn retry_push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        if self.retry_policy.idempotent_only {
            return self.inner.push_objects(store, ids).await;
        }

        let attempts = self.retry_policy.attempts();
        let mut last_err = None;

        for attempt in 1..=attempts {
            match self.inner.push_objects(store, ids).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    let retryable = self.should_retry(&err);
                    last_err = Some(err);
                    if !retryable || attempt == attempts {
                        break;
                    }
                    Self::retry_wait(self.retry_policy, attempt).await;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            SyncError::ConnectionFailed("retry loop exited without recording an error".to_string())
        }))
    }

    pub async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
        self.retry_hello().await
    }

    pub async fn advertise_refs(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, ObjectId)>, SyncError> {
        self.retry_advertise_refs(prefix).await
    }

    pub async fn fetch_objects(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
    ) -> Result<Vec<ObjectId>, SyncError> {
        self.retry_fetch_objects(store, want, have, None).await
    }

    pub async fn fetch_objects_filtered(
        &mut self,
        store: &ClawStore,
        want: &[ObjectId],
        have: &[ObjectId],
        filter: Option<PartialCloneFilter>,
    ) -> Result<Vec<ObjectId>, SyncError> {
        self.retry_fetch_objects(store, want, have, filter).await
    }

    pub async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError> {
        self.retry_update_refs(updates, force, None).await
    }

    pub async fn update_refs_with_context(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
        context: RefUpdateContext,
    ) -> Result<UpdateRefsResponse, SyncError> {
        self.retry_update_refs(updates, force, Some(context)).await
    }

    pub async fn push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError> {
        self.retry_push_objects(store, ids).await
    }
}

#[cfg(test)]
impl SyncClient {
    fn from_transport_for_test(inner: Box<dyn SyncTransport>, retry_policy: RetryPolicy) -> Self {
        Self {
            inner,
            retry_policy,
        }
    }
}

pub struct GrpcSyncClient {
    client: SyncServiceClient<tonic::transport::Channel>,
    bearer_token: Option<String>,
}

impl GrpcSyncClient {
    pub async fn connect(
        addr: &str,
        bearer_token: Option<String>,
        tls: Option<GrpcTlsConfig>,
    ) -> Result<Self, SyncError> {
        let channel = match tls {
            Some(tls_config) => {
                let endpoint = Endpoint::from_shared(addr.to_string())
                    .map_err(|e| SyncError::ConnectionFailed(e.to_string()))?;
                endpoint
                    .tls_config(build_client_tls_config(tls_config)?)?
                    .connect()
                    .await?
            }
            None => {
                Endpoint::from_shared(addr.to_string())
                    .map_err(|e| SyncError::ConnectionFailed(e.to_string()))?
                    .connect()
                    .await?
            }
        };
        let client = SyncServiceClient::new(channel);
        Ok(Self {
            client,
            bearer_token,
        })
    }

    #[allow(clippy::result_large_err)]
    fn with_auth<T>(&self, mut request: tonic::Request<T>) -> Result<tonic::Request<T>, SyncError> {
        if let Some(token) = &self.bearer_token {
            let auth_value = format!("Bearer {token}");
            let metadata_value = auth_value.parse().map_err(|e| {
                SyncError::TransferFailed(format!("invalid bearer token metadata: {e}"))
            })?;
            request
                .metadata_mut()
                .insert("authorization", metadata_value);
        }
        Ok(request)
    }

    #[allow(clippy::result_large_err)]
    fn with_replay_nonce<T>(
        &self,
        request: tonic::Request<T>,
    ) -> Result<tonic::Request<T>, SyncError> {
        let mut request = self.with_auth(request)?;
        let nonce = new_replay_nonce();
        let metadata_value = nonce
            .parse()
            .map_err(|e| SyncError::TransferFailed(format!("invalid replay nonce: {e}")))?;
        request
            .metadata_mut()
            .insert(REPLAY_NONCE_METADATA_KEY, metadata_value);
        Ok(request)
    }
}

#[allow(clippy::result_large_err)]
fn build_client_tls_config(config: GrpcTlsConfig) -> Result<ClientTlsConfig, SyncError> {
    let mut tls = ClientTlsConfig::new();
    if let Some(domain_name) = config.domain_name {
        tls = tls.domain_name(domain_name);
    }
    if let Some(ca_cert_pem) = config.ca_cert_pem {
        tls = tls.ca_certificate(Certificate::from_pem(ca_cert_pem));
    }
    match (config.client_cert_pem, config.client_key_pem) {
        (Some(cert), Some(key)) => {
            tls = tls.identity(Identity::from_pem(cert, key));
        }
        (None, None) => {}
        _ => {
            return Err(SyncError::ConnectionFailed(
                "client TLS requires both client certificate and client key".to_string(),
            ));
        }
    }
    Ok(tls)
}

fn new_replay_nonce() -> String {
    let bytes: [u8; 16] = rand::thread_rng().gen();
    hex::encode(bytes)
}

#[async_trait]
impl SyncTransport for GrpcSyncClient {
    async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
        let request = self.with_auth(tonic::Request::new(HelloRequest {
            client_version: SYNC_PROTOCOL_VERSION.to_string(),
            capabilities: server_capabilities(),
        }))?;
        let resp = self.client.hello(request).await?;
        Ok(resp.into_inner())
    }

    async fn advertise_refs(&mut self, prefix: &str) -> Result<Vec<(String, ObjectId)>, SyncError> {
        let request = self.with_auth(tonic::Request::new(AdvertiseRefsRequest {
            prefix: prefix.to_string(),
        }))?;
        let resp = self.client.advertise_refs(request).await?;
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
        filter: Option<PartialCloneFilter>,
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
            .fetch_objects(self.with_auth(tonic::Request::new(FetchObjectsRequest {
                want: want_msgs,
                have: have_msgs,
                filter,
            }))?)
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
            .update_refs(
                self.with_replay_nonce(tonic::Request::new(UpdateRefsRequest {
                    updates: proto_updates,
                }))?,
            )
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
        let request = self.with_replay_nonce(tonic::Request::new(stream))?;
        let resp = self.client.push_objects(request).await?;
        Ok(resp.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    struct MockTransport {
        hello_remaining_failures: Arc<Mutex<u32>>,
        update_refs_calls: Arc<Mutex<u32>>,
    }

    impl MockTransport {
        fn new(hello_remaining_failures: u32) -> Self {
            Self {
                hello_remaining_failures: Arc::new(Mutex::new(hello_remaining_failures)),
                update_refs_calls: Arc::new(Mutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl SyncTransport for MockTransport {
        async fn hello(&mut self) -> Result<HelloResponse, SyncError> {
            let mut remaining = self.hello_remaining_failures.lock().await;
            if *remaining > 0 {
                *remaining -= 1;
                return Err(SyncError::Grpc(tonic::Status::unavailable("temporary")));
            }

            Ok(HelloResponse {
                server_version: "0.1.0".to_string(),
                capabilities: vec![],
            })
        }

        async fn advertise_refs(
            &mut self,
            _prefix: &str,
        ) -> Result<Vec<(String, ObjectId)>, SyncError> {
            Ok(Vec::new())
        }

        async fn fetch_objects(
            &mut self,
            _store: &ClawStore,
            _want: &[ObjectId],
            _have: &[ObjectId],
            _filter: Option<PartialCloneFilter>,
        ) -> Result<Vec<ObjectId>, SyncError> {
            Ok(Vec::new())
        }

        async fn update_refs(
            &mut self,
            _updates: &[(String, Option<ObjectId>, ObjectId)],
            _force: bool,
        ) -> Result<UpdateRefsResponse, SyncError> {
            let mut calls = self.update_refs_calls.lock().await;
            *calls += 1;
            Err(SyncError::Grpc(tonic::Status::unavailable(
                "non-idempotent should not retry",
            )))
        }

        async fn push_objects(
            &mut self,
            _store: &ClawStore,
            _ids: &[ObjectId],
        ) -> Result<PushObjectsResponse, SyncError> {
            Ok(PushObjectsResponse {
                success: true,
                message: "ok".to_string(),
                accepted: vec![],
            })
        }
    }

    #[tokio::test]
    async fn retries_idempotent_hello_until_success() {
        let transport = Box::new(MockTransport::new(2));
        let mut client = SyncClient::from_transport_for_test(
            transport,
            RetryPolicy {
                idempotent_only: true,
                max_attempts: 4,
                base_backoff_ms: 1,
                max_backoff_ms: 4,
                jitter: false,
            },
        );

        let hello = client
            .hello()
            .await
            .expect("hello should eventually succeed");
        assert_eq!(hello.server_version, "0.1.0");
    }

    #[tokio::test]
    async fn does_not_retry_non_idempotent_update_refs() {
        let transport = MockTransport::new(0);
        let calls = Arc::clone(&transport.update_refs_calls);
        let mut client = SyncClient::from_transport_for_test(
            Box::new(transport),
            RetryPolicy {
                idempotent_only: true,
                max_attempts: 5,
                base_backoff_ms: 1,
                max_backoff_ms: 4,
                jitter: false,
            },
        );

        let err = client
            .update_refs(&[], false)
            .await
            .expect_err("update_refs should fail immediately");
        match err {
            SyncError::Grpc(status) => assert_eq!(status.code(), Code::Unavailable),
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(*calls.lock().await, 1);
    }

    #[tokio::test]
    async fn retries_non_idempotent_update_refs_when_explicitly_enabled() {
        let transport = MockTransport::new(0);
        let calls = Arc::clone(&transport.update_refs_calls);
        let mut client = SyncClient::from_transport_for_test(
            Box::new(transport),
            RetryPolicy {
                idempotent_only: false,
                max_attempts: 3,
                base_backoff_ms: 1,
                max_backoff_ms: 4,
                jitter: false,
            },
        );

        let err = client
            .update_refs(&[], false)
            .await
            .expect_err("update_refs should fail after configured attempts");
        match err {
            SyncError::Grpc(status) => assert_eq!(status.code(), Code::Unavailable),
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(*calls.lock().await, 3);
    }
}
