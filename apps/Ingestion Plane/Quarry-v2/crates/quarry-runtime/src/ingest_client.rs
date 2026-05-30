//! Data Plane ingest client — posts DataPlaneIngestRequest to the data plane.
//!
//! Two transports are supported:
//! - HTTP (`POST /v1/ingest`) for simple deployments
//! - gRPC (`DocumentService.CreateDocument`) for prod deployments wired into
//!   Data Plane v2 (gated behind the `grpc` feature)
//!
//! ZDR enforcement happens *before* the network call: when the request
//! declares `zdr: ZdrMode::On`, the client refuses to send `markdown` or
//! `html_ref`/`raw_ref` payloads (only refs to ephemeral artifacts are
//! permissible). This is belt-and-braces alongside the route-level guard.

use std::time::Duration;

use async_trait::async_trait;
use quarry_core::contracts::{DataPlaneIngestRequest, DataPlaneIngestResponse};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::zdr::ZdrMode;
use reqwest::Client;

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

        let url = format!("{}/v1/ingest", self.base_url.trim_end_matches('/'));

        let resp = self
            .http
            .post(&url)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| {
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

        resp.json::<DataPlaneIngestResponse>().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("ingest response parse failed: {e}"),
            )
        })
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
            source_trace: None,
        }
    }

    #[tokio::test]
    async fn ingest_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ingest"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "document_id": "doc_1",
                "index_status": "indexed",
                "knowledge_unit_count": 3,
                "embedding_status": "embedded",
                "trace_id": "trace_1",
            })))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), "test-key").unwrap();
        let resp = client.ingest(&make_request()).await.unwrap();
        assert_eq!(resp.document_id, "doc_1");
        assert_eq!(resp.index_status, IndexStatus::Indexed);
        assert_eq!(resp.embedding_status, EmbeddingStatus::Embedded);
        assert_eq!(resp.knowledge_unit_count, 3);
    }

    #[tokio::test]
    async fn ingest_rate_limited() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ingest"))
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
            .and(path("/v1/ingest"))
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
    async fn ingest_allows_zdr_with_no_payload() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "document_id": "doc_eph",
                "index_status": "skipped",
                "knowledge_unit_count": 0,
                "embedding_status": "skipped",
                "trace_id": "trace_eph",
            })))
            .mount(&server)
            .await;

        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.html_ref = None;
        req.raw_ref = None;

        let client = IngestClient::new(server.uri(), "key").unwrap();
        let resp = client.ingest(&req).await.unwrap();
        assert_eq!(resp.document_id, "doc_eph");
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
    async fn ingest_allows_zdr_with_safe_metadata() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "document_id": "doc",
                "index_status": "skipped",
                "knowledge_unit_count": 0,
                "embedding_status": "skipped",
                "trace_id": "trace_meta_safe",
            })))
            .mount(&server)
            .await;

        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.metadata = json!({
            "title": "Safe metadata",
            "language": "en",
            "fetched_at": "2026-05-08T00:00:00Z",
        });
        let client = IngestClient::new(server.uri(), "key").unwrap();
        let resp = client.ingest(&req).await.unwrap();
        assert_eq!(resp.document_id, "doc");
    }

    #[tokio::test]
    async fn ingest_403_returns_forbidden() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(403).set_body_string("zdr violation"))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), "key").unwrap();
        let err = client.ingest(&make_request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }
}
