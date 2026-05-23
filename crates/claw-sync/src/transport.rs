use async_trait::async_trait;

use claw_core::id::ObjectId;
use claw_store::ClawStore;
use serde::{Deserialize, Serialize};

use crate::proto::sync::{
    HelloResponse, PartialCloneFilter, PushObjectsResponse, UpdateRefsResponse,
};
use crate::security::redacted_secret_marker;
use crate::SyncError;

#[derive(Clone)]
pub struct GrpcTlsConfig {
    pub ca_cert_pem: Option<Vec<u8>>,
    pub client_cert_pem: Option<Vec<u8>>,
    pub client_key_pem: Option<Vec<u8>>,
    pub domain_name: Option<String>,
}

impl std::fmt::Debug for GrpcTlsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcTlsConfig")
            .field(
                "ca_cert_pem",
                &redacted_secret_marker(self.ca_cert_pem.is_some()),
            )
            .field(
                "client_cert_pem",
                &redacted_secret_marker(self.client_cert_pem.is_some()),
            )
            .field(
                "client_key_pem",
                &redacted_secret_marker(self.client_key_pem.is_some()),
            )
            .field("domain_name", &self.domain_name)
            .finish()
    }
}

#[derive(Clone)]
pub enum RemoteTransportConfig {
    Grpc {
        addr: String,
        bearer_token: Option<String>,
        tls: Option<GrpcTlsConfig>,
    },
    Http {
        base_url: String,
        repo: String,
        bearer_token: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RefUpdateContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<RefUpdatePolicyCheck>,
    #[serde(
        rename = "requestedCapabilities",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub requested_capabilities: Vec<String>,
    #[serde(
        rename = "negotiatedCapabilities",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub negotiated_capabilities: Vec<String>,
}

impl RefUpdateContext {
    pub fn has_policy_checks(&self) -> bool {
        !self.policies.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefUpdatePolicyCheck {
    pub id: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub object: String,
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl std::fmt::Debug for RemoteTransportConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Grpc {
                addr,
                bearer_token,
                tls,
            } => f
                .debug_struct("Grpc")
                .field("addr", addr)
                .field(
                    "bearer_token",
                    &redacted_secret_marker(bearer_token.is_some()),
                )
                .field("tls", tls)
                .finish(),
            Self::Http {
                base_url,
                repo,
                bearer_token,
            } => f
                .debug_struct("Http")
                .field("base_url", base_url)
                .field("repo", repo)
                .field(
                    "bearer_token",
                    &redacted_secret_marker(bearer_token.is_some()),
                )
                .finish(),
        }
    }
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
        filter: Option<PartialCloneFilter>,
    ) -> Result<Vec<ObjectId>, SyncError>;

    async fn update_refs(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
    ) -> Result<UpdateRefsResponse, SyncError>;

    async fn update_refs_with_context(
        &mut self,
        updates: &[(String, Option<ObjectId>, ObjectId)],
        force: bool,
        _context: Option<RefUpdateContext>,
    ) -> Result<UpdateRefsResponse, SyncError> {
        self.update_refs(updates, force).await
    }

    async fn push_objects(
        &mut self,
        store: &ClawStore,
        ids: &[ObjectId],
    ) -> Result<PushObjectsResponse, SyncError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_grpc_bearer_token() {
        let config = RemoteTransportConfig::Grpc {
            addr: "http://127.0.0.1:50051".to_string(),
            bearer_token: Some("super-secret-token".to_string()),
            tls: Some(GrpcTlsConfig {
                ca_cert_pem: Some(b"secret-ca".to_vec()),
                client_cert_pem: Some(b"secret-cert".to_vec()),
                client_key_pem: Some(b"secret-key".to_vec()),
                domain_name: Some("localhost".to_string()),
            }),
        };

        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret-token"));
        assert!(!rendered.contains("secret-ca"));
        assert!(!rendered.contains("secret-cert"));
        assert!(!rendered.contains("secret-key"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
