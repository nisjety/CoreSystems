//! Data Plane ingest client — posts DataPlaneIngestRequest to the data plane.
//!
//! Two transports are supported:
//! - HTTP (`POST /v1/documents`) for simple deployments
//! - gRPC (`DocumentService.CreateDocument`) for prod deployments wired into
//!   Data Plane v2 (gated behind the `grpc` feature)
//!
//! ZDR enforcement happens *before* the network call: when the request
//! declares `zdr: ZdrMode::On`, the client refuses to send `markdown` or
//! `html_ref`/`raw_ref` payloads (only refs to ephemeral artifacts are
//! permissible). This is belt-and-braces alongside the route-level guard.

use std::time::Duration;

use async_trait::async_trait;
use quarry_core::contracts::{
    DataPlaneIngestRequest, DataPlaneIngestResponse, EmbeddingStatus, IndexStatus,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::privacy::{PrivacyClassification, PrivacyPolicy};
use quarry_core::zdr::ZdrMode;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Transport-agnostic Data Plane ingest interface. P2 / cluster #grpc.
///
/// PageRunner holds an `Option<Arc<dyn DataPlaneIngest>>` so the same
/// runtime code paths work whether the deployment talks to Data Plane
/// over HTTP/1.1+JSON (via `IngestClient`) or HTTP/2+protobuf (via
/// `GrpcIngestAdapter`, gated behind the `grpc` feature). Both impls
/// enforce the same ZDR pre-check before any wire I/O so a misconfigured
/// transport cannot smuggle content past the guard.
#[async_trait]
pub trait DataPlaneIngest: Send + Sync {
    async fn ingest(
        &self,
        request: &DataPlaneIngestRequest,
    ) -> QuarryResult<DataPlaneIngestResponse>;
}

pub struct IngestClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl IngestClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("ingest client: {e}")))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            api_key: api_key.into(),
        })
    }

    pub async fn ingest(
        &self,
        request: &DataPlaneIngestRequest,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        Self::pre_check_zdr(request)?;

        // ZDR=on means zero durable retention. The canonical durable-write
        // route (`POST /v1/documents`) mandates a non-empty content body and
        // persists it, so a ZDR run MUST NOT write there. Skip the durable
        // write; upstream still hands the agent the live scrape result as
        // short-lived context. Nothing persisted → Skipped.
        if request.zdr == ZdrMode::On {
            return Ok(DataPlaneIngestResponse {
                document_id: String::new(),
                index_status: IndexStatus::Skipped,
                knowledge_unit_count: 0,
                embedding_status: EmbeddingStatus::Skipped,
                retrievable_after: None,
                trace_id: "zdr-ephemeral-skip".to_string(),
            });
        }

        let body = CreateDocumentBody::from_request(request);
        let url = format!("{}/v1/documents", self.base_url.trim_end_matches('/'));

        let mut req_builder = self
            .http
            .post(&url)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("x-internal-api-key", &self.api_key)
            .header("x-org-id", &request.org_id)
            .header("content-type", "application/json");
        // Private-by-default ownership: forward the initiating user so
        // documents-api stamps owner=user + visibility=private. Absent →
        // system/connector ingest → org-visible (legacy behavior preserved).
        if let Some(uid) = request.initiator_user_id.as_deref() {
            if !uid.is_empty() {
                req_builder = req_builder.header("x-user-id", uid);
            }
        }
        let resp = req_builder.json(&body).send().await.map_err(|e| {
            let code = if e.is_timeout() {
                ErrorCode::Timeout
            } else if e.is_connect() {
                ErrorCode::UpstreamBlocked
            } else {
                ErrorCode::DriverFailed
            };
            QuarryError::new(code, format!("ingest request failed: {e}"))
        })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                if status.as_u16() == 429 {
                    ErrorCode::RateLimited
                } else if status.as_u16() == 403 {
                    ErrorCode::Forbidden
                } else if status.as_u16() >= 500 {
                    ErrorCode::DriverFailed
                } else {
                    ErrorCode::BadRequest
                },
                format!("data plane returned {}: {}", status.as_u16(), text),
            ));
        }

        // Capture the trace header before consuming the response body.
        let trace_id = resp
            .headers()
            .get("x-trace-id")
            .or_else(|| resp.headers().get("x-request-id"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let doc = resp.json::<CreateDocumentResponse>().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("ingest response parse failed: {e}"),
            )
        })?;

        Ok(map_document_response(doc, trace_id))
    }
}

#[async_trait]
impl DataPlaneIngest for IngestClient {
    async fn ingest(
        &self,
        request: &DataPlaneIngestRequest,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        // Delegate to the inherent method so the call site preserves
        // every existing behavior (ZDR pre-check, error mapping, etc.).
        IngestClient::ingest(self, request).await
    }
}

impl IngestClient {
    /// ZDR pre-check — blocks attempts to ship durable content when the run
    /// declared ZDR. Returns `Forbidden` typed error so callers can react.
    ///
    /// The check covers four leak vectors:
    /// 1. `markdown` field — explicit content
    /// 2. `html_ref` / `raw_ref` — artifact ID pointers (Data Plane could
    ///    fetch the artifact and persist content from the bytes)
    /// 3. `chunks` — pre-chunked content with text bodies
    /// 4. `metadata` — JSON object that smells like content (large strings,
    ///    keys named `content`/`body`/`text`/`html`/`markdown`)
    ///
    /// The metadata check is deliberately conservative — false positives
    /// would block legitimate metadata (titles, URLs, fingerprints), so we
    /// only flag fields whose key OR value clearly carries content-shaped
    /// data above a length threshold.
    fn pre_check_zdr(request: &DataPlaneIngestRequest) -> QuarryResult<()> {
        if request.zdr != ZdrMode::On {
            return Ok(());
        }
        if request.markdown.is_some() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "ZDR=on rejects ingest with markdown payload (use ephemeral path or strip)",
            ));
        }
        if request.html_ref.is_some() || request.raw_ref.is_some() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "ZDR=on rejects ingest with html/raw artifact refs",
            ));
        }
        if !request.chunks.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "ZDR=on rejects ingest with chunks payload (chunks carry document content)",
            ));
        }
        if let Some(violation) = scan_metadata_for_content(&request.metadata) {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                format!("ZDR=on metadata smells like content: {violation}"),
            ));
        }
        Ok(())
    }
}

/// Scan a metadata JSON value for keys/values that look like content.
/// Returns a human-readable description of the first violation, or `None`
/// when the metadata is safe.
///
/// The heuristic is intentionally narrow — production deployments must
/// not block on false positives. We only flag:
/// - Keys named `content`/`body`/`text`/`html`/`markdown`/`raw` (any case)
/// - Strings longer than `MAX_METADATA_STRING_LEN` (1024 chars)
fn scan_metadata_for_content(meta: &serde_json::Value) -> Option<String> {
    const SUSPICIOUS_KEYS: &[&str] = &["content", "body", "text", "html", "markdown", "raw"];
    const MAX_METADATA_STRING_LEN: usize = 1024;

    fn walk(path: &str, value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let key_lower = k.to_lowercase();
                    if SUSPICIOUS_KEYS.contains(&key_lower.as_str()) {
                        return Some(format!("`{path}.{k}` is a content-shaped key"));
                    }
                    if let Some(violation) = walk(&format!("{path}.{k}"), v) {
                        return Some(violation);
                    }
                }
                None
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    if let Some(violation) = walk(&format!("{path}[{i}]"), v) {
                        return Some(violation);
                    }
                }
                None
            }
            serde_json::Value::String(s) => {
                if s.chars().count() > MAX_METADATA_STRING_LEN {
                    Some(format!(
                        "`{path}` carries a {}-char string (>{} chars suggests content body)",
                        s.chars().count(),
                        MAX_METADATA_STRING_LEN
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    walk("metadata", meta)
}

// ---------------------------------------------------------------------------
// Wire mapping: Quarry `DataPlaneIngestRequest` → Data Plane v2 Documents API
// (`POST /v1/documents`, body shape `model.CreateDocumentInput`). Chunks are
// derived downstream by the index-engine, so they are NOT sent inline.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CreateDocumentBody {
    org_id: String,
    source: &'static str,
    #[serde(rename = "type")]
    doc_type: &'static str,
    title: String,
    content: String,
    metadata: serde_json::Value,
    /// GDPR/ZDR classification on Data Plane v2's allow-list
    /// (`internal` | `public` | `sensitive` | `restricted`). Derived from the
    /// upstream `PrivacyPolicy.privacy_classification` so the real classification
    /// computed by Quarry actually reaches the document row — previously this was
    /// dropped, leaving documents-api to hardcode `zdr_classification='internal'`
    /// and rendering retrieval-engine's `reject`-mode restricted-content filter
    /// (which keys off `zdr_classification='restricted'`) dead for Quarry traffic.
    /// Empty is skipped so the receiver's own default still applies when no policy
    /// was computed.
    #[serde(skip_serializing_if = "String::is_empty")]
    zdr_classification: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    extraction_trace: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "String::is_empty")]
    idempotency_key: String,
}

/// Map Quarry's `PrivacyClassification` onto Data Plane v2's `zdr_classification`
/// allow-list (`internal` | `public` | `sensitive` | `restricted`, validated in
/// documents-api-go `internal/validate`). This is the load-bearing translation
/// that makes retrieval-engine's restricted-content enforcement operate on real
/// data: `CredentialOrSecret` and `ZdrEphemeral` map to `restricted` so they are
/// filtered out of `reject`-mode retrieval, personal data maps to `sensitive`,
/// and the default `CustomerPrivate` maps to `internal` (matching the receiver's
/// prior default, so callers that set no policy see no behavior change).
fn zdr_classification_for(policy: Option<&PrivacyPolicy>) -> &'static str {
    let classification = policy.map(|p| p.privacy_classification).unwrap_or_default();
    match classification {
        PrivacyClassification::PublicNonPersonal => "public",
        PrivacyClassification::CustomerPrivate => "internal",
        PrivacyClassification::Personal | PrivacyClassification::SensitivePersonal => "sensitive",
        PrivacyClassification::CredentialOrSecret | PrivacyClassification::ZdrEphemeral => {
            "restricted"
        }
    }
}

impl CreateDocumentBody {
    fn from_request(request: &DataPlaneIngestRequest) -> Self {
        // Data Plane v2 dedupes Quarry rows on `(org_id, metadata->>'url')`
        // (partial unique index where `source='quarry'`), so the canonical URL
        // MUST ride in metadata. fingerprint/run_id travel alongside for
        // provenance without clobbering any caller-supplied metadata.
        let mut meta = match &request.metadata {
            serde_json::Value::Object(m) => m.clone(),
            _ => serde_json::Map::new(),
        };
        meta.insert(
            "url".to_string(),
            serde_json::Value::String(request.source_url.clone()),
        );
        meta.entry("fingerprint".to_string())
            .or_insert_with(|| serde_json::Value::String(request.fingerprint.clone()));
        if let Ok(run_id) = serde_json::to_value(&request.run_id) {
            meta.insert("run_id".to_string(), run_id);
        }

        // Preserve the FULL GDPR policy contract (purpose_id, lawful_basis,
        // retention_policy, residency, allow_third_party_processing,
        // processor_id, ...) in metadata for audit/DSAR provenance. documents-api
        // only has a first-class column for the classification (mapped onto
        // `zdr_classification` below); the remaining fields ride in metadata so
        // the contract Quarry computes survives the wire hop instead of being
        // silently discarded.
        if let Some(policy) = &request.privacy_policy {
            if let Ok(policy_value) = serde_json::to_value(policy) {
                meta.insert("privacy_policy".to_string(), policy_value);
            }
        }

        // Title is required & non-empty at the Data Plane boundary; fall back
        // to the source URL when the page yielded no title.
        let title = request
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| request.source_url.clone());

        let extraction_trace = request
            .source_trace
            .as_ref()
            .and_then(|t| serde_json::to_value(t).ok());

        Self {
            org_id: request.org_id.clone(),
            source: "quarry",
            doc_type: "web_page",
            title,
            content: request.markdown.clone().unwrap_or_default(),
            metadata: serde_json::Value::Object(meta),
            zdr_classification: zdr_classification_for(request.privacy_policy.as_ref()).to_string(),
            extraction_trace,
            // A URL-stable idempotency key (NOT the content fingerprint) makes
            // re-ingest correct: Data Plane looks up `(org_id, key)` and, on a
            // content change, UPDATEs the same row in place (re-index/re-embed)
            // rather than inserting a duplicate that would collide with the
            // `(org_id, metadata->>'url')` crawl-dedup unique index. The content
            // fingerprint still rides in metadata for change tracking.
            idempotency_key: format!(
                "quarry-url:{}",
                blake3::hash(request.source_url.as_bytes()).to_hex()
            ),
        }
    }
}

/// Subset of Data Plane v2's `model.Document` create response we consume.
/// Unknown fields are ignored by serde.
#[derive(Deserialize)]
struct CreateDocumentResponse {
    document_id: String,
    #[serde(default)]
    status: String,
}

/// Map the Data Plane document-create response onto the transport-agnostic
/// `DataPlaneIngestResponse`. Chunking + embedding run async downstream, so a
/// fresh write is `Pending` on both axes and the knowledge-unit count is not
/// yet known (0); retrieval readiness is signalled separately by Data Plane.
fn map_document_response(doc: CreateDocumentResponse, trace_id: String) -> DataPlaneIngestResponse {
    let index_status = match doc.status.as_str() {
        "indexed" => IndexStatus::Indexed,
        "failed" => IndexStatus::Failed,
        "skipped" => IndexStatus::Skipped,
        _ => IndexStatus::Pending,
    };
    DataPlaneIngestResponse {
        document_id: doc.document_id,
        index_status,
        knowledge_unit_count: 0,
        embedding_status: EmbeddingStatus::Pending,
        retrievable_after: None,
        trace_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::contracts::{EmbeddingStatus, IndexStatus};
    use quarry_core::ids::Id;
    use quarry_core::zdr::ZdrMode;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_request() -> DataPlaneIngestRequest {
        DataPlaneIngestRequest {
            run_id: Id::new(),
            org_id: "org_test".into(),
            source_url: "https://example.com".into(),
            title: Some("Test".into()),
            markdown: Some("# Hello".into()),
            html_ref: None,
            raw_ref: None,
            chunks: vec![],
            metadata: json!({}),
            fingerprint: "blake3:abc".into(),
            zdr: ZdrMode::Off,
            retention_policy: None,
            privacy_policy: Some(quarry_core::privacy::PrivacyPolicy::default()),
            source_trace: None,
            initiator_user_id: None,
            visibility: None,
        }
    }

    #[tokio::test]
    async fn ingest_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .and(header("authorization", "Bearer test-key"))
            .and(header("x-internal-api-key", "test-key"))
            .and(header("x-org-id", "org_test"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "document_id": "doc_1",
                "org_id": "org_test",
                "source": "quarry",
                "type": "web_page",
                "title": "Test",
                "content": "# Hello",
                "status": "pending",
                "metadata": {"url": "https://example.com"},
                "zdr_classification": "internal",
                "created_at": "2026-05-08T00:00:00Z",
                "updated_at": "2026-05-08T00:00:00Z",
            })))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), "test-key").unwrap();
        let resp = client.ingest(&make_request()).await.unwrap();
        assert_eq!(resp.document_id, "doc_1");
        // Indexing + embedding run async downstream of the document write.
        assert_eq!(resp.index_status, IndexStatus::Pending);
        assert_eq!(resp.embedding_status, EmbeddingStatus::Pending);
        assert_eq!(resp.knowledge_unit_count, 0);
    }

    #[tokio::test]
    async fn ingest_rate_limited() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), "key").unwrap();
        let err = client.ingest(&make_request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn ingest_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal"))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), "key").unwrap();
        let err = client.ingest(&make_request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn ingest_blocks_zdr_with_markdown_payload() {
        let mut req = make_request();
        req.zdr = ZdrMode::On;

        let client = IngestClient::new("http://localhost:1", "key").unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("ZDR=on"));
    }

    #[tokio::test]
    async fn ingest_zdr_no_payload_skips_durable_write() {
        // ZDR=on with no durable payload: the client must NOT POST to the
        // durable route (which mandates content) — it returns Skipped without
        // any network call. Pointing at a dead address proves no request fires.
        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.html_ref = None;
        req.raw_ref = None;

        let client = IngestClient::new("http://127.0.0.1:1", "key").unwrap();
        let resp = client.ingest(&req).await.unwrap();
        assert_eq!(resp.index_status, IndexStatus::Skipped);
        assert_eq!(resp.embedding_status, EmbeddingStatus::Skipped);
        assert!(resp.document_id.is_empty());
    }

    #[tokio::test]
    async fn ingest_blocks_zdr_with_chunks_payload() {
        use quarry_core::contracts::ChunkRef;

        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.chunks = vec![ChunkRef {
            start: 0,
            end: 100,
            text: "chunk content".into(),
        }];

        let client = IngestClient::new("http://localhost:1", "key").unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("chunks"));
    }

    #[tokio::test]
    async fn ingest_blocks_zdr_with_content_keyed_metadata() {
        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.metadata = json!({
            "title": "Article",
            "body": "this is the article body smuggled into metadata",
        });
        let client = IngestClient::new("http://localhost:1", "key").unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("smells like content"));
    }

    #[tokio::test]
    async fn ingest_blocks_zdr_with_oversized_metadata_string() {
        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.metadata = json!({
            "summary": "x".repeat(2_000),
        });
        let client = IngestClient::new("http://localhost:1", "key").unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn ingest_zdr_safe_metadata_passes_precheck_then_skips() {
        // Safe (non-content) metadata clears the ZDR pre-check; the durable
        // write is still skipped because ZDR=on means zero retention.
        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.metadata = json!({
            "title": "Safe metadata",
            "language": "en",
            "fetched_at": "2026-05-08T00:00:00Z",
        });
        let client = IngestClient::new("http://127.0.0.1:1", "key").unwrap();
        let resp = client.ingest(&req).await.unwrap();
        assert_eq!(resp.index_status, IndexStatus::Skipped);
    }

    #[tokio::test]
    async fn ingest_403_returns_forbidden() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(403).set_body_string("zdr violation"))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), "key").unwrap();
        let err = client.ingest(&make_request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[test]
    fn from_request_maps_quarry_fields_with_url_stable_idempotency_key() {
        let mut req = make_request();
        req.title = None; // exercise the title → source_url fallback
        req.markdown = Some("# Body".into());
        let body = CreateDocumentBody::from_request(&req);
        let v = serde_json::to_value(&body).unwrap();

        assert_eq!(v["source"], "quarry");
        assert_eq!(v["type"], "web_page");
        assert_eq!(v["content"], "# Body");
        assert_eq!(v["title"], "https://example.com"); // fell back to source_url
        assert_eq!(v["metadata"]["url"], "https://example.com");
        // Idempotency key is derived from the URL (stable across content
        // changes), NOT the content fingerprint, so re-ingest UPDATEs in place.
        let key = v["idempotency_key"].as_str().unwrap();
        assert!(key.starts_with("quarry-url:"));
        assert_ne!(key, "blake3:abc"); // not the content fingerprint

        // The default policy (CustomerPrivate) maps to "internal" and the full
        // policy contract is preserved in metadata for audit/DSAR provenance.
        assert_eq!(v["zdr_classification"], "internal");
        assert_eq!(
            v["metadata"]["privacy_policy"]["privacy_classification"],
            "customer_private"
        );
    }

    #[test]
    fn zdr_classification_maps_privacy_classification_to_data_plane_allowlist() {
        use quarry_core::privacy::PrivacyClassification as C;
        // (Quarry classification, Data Plane allow-list value). The allow-list is
        // {internal, public, sensitive, restricted} per documents-api-go validate.
        let cases = [
            (C::PublicNonPersonal, "public"),
            (C::CustomerPrivate, "internal"),
            (C::Personal, "sensitive"),
            (C::SensitivePersonal, "sensitive"),
            (C::CredentialOrSecret, "restricted"),
            (C::ZdrEphemeral, "restricted"),
        ];
        for (classification, expected) in cases {
            let policy = PrivacyPolicy {
                privacy_classification: classification,
                ..PrivacyPolicy::default()
            };
            assert_eq!(
                zdr_classification_for(Some(&policy)),
                expected,
                "classification {classification:?} should map to {expected}"
            );
        }
        // No policy → the receiver's own default still applies (empty, skipped).
        assert_eq!(zdr_classification_for(None), "internal");
    }

    #[test]
    fn from_request_carries_restricted_classification_for_credentials() {
        // A CredentialOrSecret document MUST reach documents-api as "restricted"
        // so retrieval-engine's reject-mode filter (WHERE zdr_classification =
        // 'restricted') actually removes it. Before this fix the field was
        // dropped and the row defaulted to "internal" — never filtered.
        let mut req = make_request();
        req.privacy_policy = Some(PrivacyPolicy {
            privacy_classification: PrivacyClassification::CredentialOrSecret,
            purpose_id: Some("support_triage".into()),
            lawful_basis: Some("legitimate_interest".into()),
            residency: Some("eu".into()),
            ..PrivacyPolicy::default()
        });
        let body = CreateDocumentBody::from_request(&req);
        let v = serde_json::to_value(&body).unwrap();

        assert_eq!(v["zdr_classification"], "restricted");
        // Full contract survives in metadata.
        assert_eq!(
            v["metadata"]["privacy_policy"]["purpose_id"],
            "support_triage"
        );
        assert_eq!(
            v["metadata"]["privacy_policy"]["lawful_basis"],
            "legitimate_interest"
        );
        assert_eq!(v["metadata"]["privacy_policy"]["residency"], "eu");
    }
}
