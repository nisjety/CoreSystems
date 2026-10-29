//! gRPC client for Data Plane v2 `DocumentService`.
//!
//! Quarry calls this when scrape requests opt into Data Plane ingest. The
//! client owns: org-scoped CreateDocument, BulkIngest, GetDocumentIndexStatus,
//! GetIngestStatus.
//!
//! ZDR semantics:
//! - When the scrape request has ZDR on, callers MUST set `zdr_classification`
//!   to "ephemeral" and use an `IngestPolicy { ephemeral_only: true }` so the
//!   Data Plane side won't persist content.
//! - Quarry preserves the typed Forbidden error if Data Plane rejects.

use std::time::Duration;

use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;

use crate::ingest_client::{ensure_durable_ingest_allowed, validate_data_plane_bearer};

use super::browser_broker::grpc_status_to_error;
use super::dataplane::documents::v2::document_service_client::DocumentServiceClient;
use super::dataplane::documents::v2::{
    BulkIngestRequest, CreateDocumentRequest, CreateDocumentResponse,
    GetDocumentIndexStatusRequest, GetDocumentIndexStatusResponse, IngestPolicy,
    IngestStatusRequest, IngestStatusResponse,
};

#[derive(Debug, Clone)]
pub struct DataPlaneIngestPolicy {
    pub zdr_mode: String,
    pub index_schedule: String,
    pub ephemeral_only: bool,
}

impl DataPlaneIngestPolicy {
    pub fn ephemeral() -> Self {
        Self {
            zdr_mode: "on".into(),
            index_schedule: "ephemeral".into(),
            ephemeral_only: true,
        }
    }

    pub fn standard() -> Self {
        Self {
            zdr_mode: "off".into(),
            index_schedule: "default".into(),
            ephemeral_only: false,
        }
    }

    fn into_proto(self) -> IngestPolicy {
        IngestPolicy {
            zdr_mode: self.zdr_mode,
            index_schedule: self.index_schedule,
            ephemeral_only: self.ephemeral_only,
        }
    }

    fn ensure_durable(&self) -> QuarryResult<()> {
        match (self.zdr_mode.as_str(), self.ephemeral_only) {
            ("off", false) => ensure_durable_ingest_allowed(ZdrMode::Off),
            ("on", _) => ensure_durable_ingest_allowed(ZdrMode::On),
            _ => Err(QuarryError::new(
                ErrorCode::Forbidden,
                "ambiguous Data Plane ingest policy rejected",
            )),
        }
    }
}

#[derive(Clone)]
pub struct GrpcDataPlaneClient {
    channel: Channel,
    auth_token: Option<String>,
}

impl GrpcDataPlaneClient {
    pub async fn connect(endpoint: impl Into<String>) -> QuarryResult<Self> {
        let endpoint_str = endpoint.into();
        let mut ep = Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("invalid grpc endpoint {endpoint_str}: {e}"),
            )
        })?;
        ep = ep
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            // Keep-alive — same posture as browser-broker for consistency.
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true)
            .tcp_keepalive(Some(Duration::from_secs(60)));

        if endpoint_str.starts_with("https://") {
            ep = ep
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("tls config failed: {e}"))
                })?;
        }

        let channel = ep.connect().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("grpc connect failed: {e}"))
        })?;

        Ok(Self {
            channel,
            auth_token: None,
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> QuarryResult<Self> {
        let token = token.into();
        validate_data_plane_bearer(&token)?;
        self.auth_token = Some(token);
        Ok(self)
    }

    fn client(&self) -> DocumentServiceClient<Channel> {
        DocumentServiceClient::new(self.channel.clone())
    }

    fn apply_auth<T>(&self, mut req: tonic::Request<T>) -> QuarryResult<tonic::Request<T>> {
        let token = self.auth_token.as_deref().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::Forbidden,
                "data plane gRPC requires a signed bearer token",
            )
        })?;
        validate_data_plane_bearer(token)?;
        let value = format!("Bearer {token}").parse().map_err(|_| {
            QuarryError::new(ErrorCode::Forbidden, "invalid data plane bearer metadata")
        })?;
        req.metadata_mut().insert("authorization", value);
        Ok(req)
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the generated CreateDocument RPC fields.
    pub async fn create_document(
        &self,
        org_id: &str,
        source: &str,
        type_: &str,
        title: &str,
        content: &str,
        zdr_classification: &str,
        ingest_policy: Option<DataPlaneIngestPolicy>,
    ) -> QuarryResult<CreatedDocument> {
        if org_id.is_empty() {
            return Err(QuarryError::new(ErrorCode::BadRequest, "org_id required"));
        }
        let ingest_policy = ingest_policy.ok_or_else(|| {
            QuarryError::new(
                ErrorCode::Forbidden,
                "explicit standard ingest policy required for durable create",
            )
        })?;
        ingest_policy.ensure_durable()?;
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(CreateDocumentRequest {
            org_id: org_id.to_string(),
            source: source.to_string(),
            r#type: type_.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            metadata: None,
            zdr_classification: zdr_classification.to_string(),
            ingest_policy: Some(ingest_policy.into_proto()),
        }))?;
        let resp: CreateDocumentResponse = client
            .create_document(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        let doc = resp.document.ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "data plane returned no document in CreateDocumentResponse",
            )
        })?;
        Ok(CreatedDocument {
            document_id: doc.document_id,
            status: doc.status,
            zdr_classification: doc.zdr_classification,
            zdr_reason: doc.zdr_reason,
        })
    }

    pub async fn bulk_ingest(
        &self,
        org_id: &str,
        documents: Vec<CreateDocumentRequest>,
        ingest_policy: Option<DataPlaneIngestPolicy>,
    ) -> QuarryResult<BulkIngestResult> {
        if org_id.is_empty() {
            return Err(QuarryError::new(ErrorCode::BadRequest, "org_id required"));
        }
        let ingest_policy = ingest_policy.ok_or_else(|| {
            QuarryError::new(
                ErrorCode::Forbidden,
                "explicit standard ingest policy required for durable bulk ingest",
            )
        })?;
        ingest_policy.ensure_durable()?;
        validate_bulk_scope(org_id, &documents)?;
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(BulkIngestRequest {
            org_id: org_id.to_string(),
            documents,
            ingest_policy: Some(ingest_policy.into_proto()),
        }))?;
        let resp = client
            .bulk_ingest(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        Ok(BulkIngestResult {
            accepted: resp.accepted,
            rejected: resp.rejected,
            document_ids: resp.document_ids,
        })
    }

    pub async fn get_index_status(
        &self,
        document_id: &str,
        org_id: &str,
    ) -> QuarryResult<DocumentIndexStatus> {
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(GetDocumentIndexStatusRequest {
            document_id: document_id.to_string(),
            org_id: org_id.to_string(),
        }))?;
        let resp: GetDocumentIndexStatusResponse = client
            .get_document_index_status(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        let s = resp.status.ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "data plane returned no status in GetDocumentIndexStatusResponse",
            )
        })?;
        Ok(DocumentIndexStatus {
            document_id: s.document_id,
            chunk_count: s.chunk_count,
            chunk_status: s.chunk_status,
            embed_status: s.embed_status,
            embeddings_synced: s.embeddings_synced,
            vector_status: s.vector_status,
            error_message: s.error_message,
        })
    }

    pub async fn ingest_status(&self, org_id: &str) -> QuarryResult<IngestStatus> {
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(IngestStatusRequest {
            org_id: org_id.to_string(),
        }))?;
        let resp: IngestStatusResponse = client
            .get_ingest_status(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        let policy = resp.policy.ok_or_else(|| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                "data plane returned no policy in IngestStatusResponse",
            )
        })?;
        Ok(IngestStatus {
            policy: DataPlaneIngestPolicy {
                zdr_mode: policy.zdr_mode,
                index_schedule: policy.index_schedule,
                ephemeral_only: policy.ephemeral_only,
            },
            is_active: resp.is_active,
        })
    }
}

fn validate_bulk_scope(org_id: &str, documents: &[CreateDocumentRequest]) -> QuarryResult<()> {
    for document in documents {
        if document.org_id != org_id {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "bulk document tenant must match the outer tenant",
            ));
        }
        if let Some(policy) = &document.ingest_policy {
            DataPlaneIngestPolicy {
                zdr_mode: policy.zdr_mode.clone(),
                index_schedule: policy.index_schedule.clone(),
                ephemeral_only: policy.ephemeral_only,
            }
            .ensure_durable()?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CreatedDocument {
    pub document_id: String,
    pub status: String,
    pub zdr_classification: String,
    pub zdr_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BulkIngestResult {
    pub accepted: i32,
    pub rejected: i32,
    pub document_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DocumentIndexStatus {
    pub document_id: String,
    pub chunk_count: i32,
    pub chunk_status: String,
    pub embed_status: String,
    pub embeddings_synced: i32,
    pub vector_status: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IngestStatus {
    pub policy: DataPlaneIngestPolicy,
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ephemeral_policy_sets_zdr_on() {
        let p = DataPlaneIngestPolicy::ephemeral();
        assert_eq!(p.zdr_mode, "on");
        assert!(p.ephemeral_only);
    }

    #[test]
    fn standard_policy_sets_zdr_off() {
        let p = DataPlaneIngestPolicy::standard();
        assert_eq!(p.zdr_mode, "off");
        assert!(!p.ephemeral_only);
    }

    #[test]
    fn durable_rpc_rejects_ephemeral_and_ambiguous_policy() {
        assert!(DataPlaneIngestPolicy::ephemeral().ensure_durable().is_err());
        assert!(DataPlaneIngestPolicy {
            zdr_mode: "off".into(),
            index_schedule: "default".into(),
            ephemeral_only: true,
        }
        .ensure_durable()
        .is_err());
        assert!(DataPlaneIngestPolicy::standard().ensure_durable().is_ok());
    }

    #[test]
    fn grpc_auth_rejects_missing_or_shared_key_bearer() {
        assert!(validate_data_plane_bearer("").is_err());
        assert!(validate_data_plane_bearer("shared-key").is_err());
        assert!(validate_data_plane_bearer(
            "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJzZXJ2aWNlOnF1YXJyeSJ9.c2lnbmF0dXJl"
        )
        .is_ok());
        assert!(validate_data_plane_bearer("eyJhbGciOiJub25lIn0.e30.signature").is_err());
    }

    #[test]
    fn bulk_scope_rejects_cross_tenant_and_zdr_child() {
        let standard = IngestPolicy {
            zdr_mode: "off".into(),
            index_schedule: "default".into(),
            ephemeral_only: false,
        };
        let make_document = |org_id: &str, policy: IngestPolicy| CreateDocumentRequest {
            org_id: org_id.into(),
            source: "quarry".into(),
            r#type: "web".into(),
            title: "title".into(),
            content: "content".into(),
            metadata: None,
            zdr_classification: "internal".into(),
            ingest_policy: Some(policy),
        };

        assert!(validate_bulk_scope("org-a", &[make_document("org-b", standard.clone())]).is_err());
        assert!(validate_bulk_scope(
            "org-a",
            &[make_document(
                "org-a",
                IngestPolicy {
                    zdr_mode: "on".into(),
                    index_schedule: "ephemeral".into(),
                    ephemeral_only: true,
                }
            )]
        )
        .is_err());
        assert!(validate_bulk_scope("org-a", &[make_document("org-a", standard)]).is_ok());
    }
}
