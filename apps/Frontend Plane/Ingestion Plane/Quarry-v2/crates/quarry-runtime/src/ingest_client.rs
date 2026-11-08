//! Data Plane ingest client — posts DataPlaneIngestRequest to the data plane.
//!
//! Two transports are supported:
//! - HTTP (`POST /v1/documents`) for simple deployments
//! - gRPC (`DocumentService.CreateDocument`) for prod deployments wired into
//!   Data Plane v2 (gated behind the `grpc` feature)
//!
//! ZDR enforcement happens *before* the network call: when the request
//! declares `zdr: ZdrMode::On`, the client refuses the durable endpoint before
//! wire I/O. Content-shaped fields receive a more specific rejection first.
//! This is belt-and-braces alongside the route-level guard.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use quarry_core::contracts::{
    DataPlaneIngestRequest, DataPlaneIngestResponse, EmbeddingStatus, IndexStatus,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::privacy::{PrivacyClassification, PrivacyPolicy};
use quarry_core::zdr::{self, WriteKind, ZdrMode};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::service_tokens::{
    ServiceTokenProvider, ServiceTokenRequest, SharedServiceTokenProvider,
};

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
    auth: IngestAuth,
}

enum IngestAuth {
    StaticDev(String),
    Dynamic(SharedServiceTokenProvider),
}

impl IngestClient {
    pub fn new(base_url: impl Into<String>, bearer_token: impl Into<String>) -> QuarryResult<Self> {
        let bearer_token = bearer_token.into();
        validate_data_plane_bearer(&bearer_token)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("ingest client: {}", e.without_url())))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            auth: IngestAuth::StaticDev(bearer_token),
        })
    }

    /// Production constructor. The provider mints an `aud=data-plane` token
    /// for the request's already-verified `org_id`; no bearer is retained in
    /// configuration or shared across tenants.
    pub fn with_token_provider(
        base_url: impl Into<String>,
        provider: Arc<dyn ServiceTokenProvider>,
    ) -> QuarryResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                QuarryError::new(ErrorCode::Internal, format!("ingest client: {}", error.without_url()))
            })?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            auth: IngestAuth::Dynamic(provider),
        })
    }

    pub async fn ingest(
        &self,
        request: &DataPlaneIngestRequest,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        Self::pre_check_zdr(request)?;
        ensure_durable_ingest_allowed(request.zdr)?;

        // `org:data:read_all` marks this as a SYSTEM/connector ingest, not an
        // interactive end-user write. documents-api's applyVisibilityPolicy
        // reads it (via viewerID) to stamp crawled pages `visibility=org` so the
        // whole tenant sees the crawled knowledge base. Without it, quarry-edge
        // is treated as a plain viewer and every crawled doc lands
        // `visibility=private, owner=service:quarry-edge` — persisted but
        // invisible to every real user session (owner=self OR visibility=org
        // matches neither). The scope is granted to the quarry-edge principal in
        // the Control Plane service-principal registry. The request itself is the
        // `ingest` row of contracts/cross-plane-token-requests.json, the one place
        // it is written down.
        let token_request = ServiceTokenRequest::from_manifest("ingest")?;
        let bearer = match &self.auth {
            IngestAuth::StaticDev(token) => token.clone(),
            IngestAuth::Dynamic(provider) => provider
                .token_for_org(&request.org_id, &token_request, false)
                .await?
                .expose()
                .to_owned(),
        };
        let mut resp = self.send(request, &bearer).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let IngestAuth::Dynamic(provider) = &self.auth {
                let refreshed = provider
                    .token_for_org(&request.org_id, &token_request, true)
                    .await?;
                resp = self.send(request, refreshed.expose()).await?;
            }
        }

        self.decode_response(resp).await
    }

    async fn send(
        &self,
        request: &DataPlaneIngestRequest,
        bearer: &str,
    ) -> QuarryResult<reqwest::Response> {
        let body = CreateDocumentBody::from_request(request);
        let url = format!("{}/v1/documents", self.base_url.trim_end_matches('/'));

        let req_builder = self
            .http
            .post(&url)
            .bearer_auth(bearer)
            .header("content-type", "application/json");
        req_builder.json(&body).send().await.map_err(|e| {
            let code = if e.is_timeout() {
                ErrorCode::Timeout
            } else if e.is_connect() {
                ErrorCode::UpstreamBlocked
            } else {
                ErrorCode::DriverFailed
            };
            QuarryError::new(code, format!("ingest request failed: {e}"))
        })
    }

    async fn decode_response(
        &self,
        resp: reqwest::Response,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        let status = resp.status();
        if !status.is_success() {
            return Err(QuarryError::new(
                match status.as_u16() {
                    429 => ErrorCode::RateLimited,
                    401 | 403 => ErrorCode::Forbidden,
                    code if code >= 500 => ErrorCode::DriverFailed,
                    _ => ErrorCode::BadRequest,
                },
                format!("data plane returned status {}", status.as_u16()),
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
                format!("ingest response parse failed: {}", e.without_url()),
            )
        })?;

        Ok(map_document_response(doc, trace_id))
    }
}

/// Durable Data Plane endpoints are never a valid ZDR transport. Both HTTP and
/// gRPC adapters call this exact guard before constructing a network request.
pub(crate) fn ensure_durable_ingest_allowed(zdr_mode: ZdrMode) -> QuarryResult<()> {
    zdr::guard(zdr_mode, WriteKind::Ingest)
}

/// Quarry is a forwarding client, not the token authority. It nevertheless
/// rejects legacy shared-key shapes and unsigned JWT headers locally so only a
/// compact RS256 bearer can reach Data Plane, where signature, issuer,
/// audience, expiry, scope, and tenant are authoritatively verified.
pub(crate) fn validate_data_plane_bearer(token: &str) -> QuarryResult<()> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "data plane ingest requires a signed bearer token",
        ));
    };
    if payload.is_empty() || signature.is_empty() {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "data plane ingest requires a signed bearer token",
        ));
    }
    let decoded = URL_SAFE_NO_PAD.decode(header).map_err(|_| {
        QuarryError::new(
            ErrorCode::Forbidden,
            "data plane bearer has an invalid protected header",
        )
    })?;
    let header: serde_json::Value = serde_json::from_slice(&decoded).map_err(|_| {
        QuarryError::new(
            ErrorCode::Forbidden,
            "data plane bearer has an invalid protected header",
        )
    })?;
    if header.get("alg").and_then(|value| value.as_str()) != Some("RS256") {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "data plane bearer must declare RS256",
        ));
    }
    Ok(())
}

#[async_trait]
impl DataPlaneIngest for IngestClient {
    async fn ingest(
        &self,
        request: &DataPlaneIngestRequest,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        // Delegate to the inherent method so the call site preserves every
        // existing behavior (ZDR guards, bearer-only auth, error mapping).
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
            doc_type: document_type_for(request),
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

// ---------------------------------------------------------------------------
// E3.4-8: governed download hand-off (Quarry → Data Plane).
//
// A file the browser downloaded (a PDF, a text file) reaches Data Plane only
// through this module's `DataPlaneIngest` transport, under the same
// `POST /v1/documents` contract a crawled page uses. There is no other path:
// no direct database write and no second HTTP client. What the hand-off adds is
// the governance in front of that transport:
//
// * the flag `QUARRY_EDGE__DOWNLOAD_HANDOFF` (default off);
// * ZDR refused first, before any field is read into an error or a request;
// * the document is stamped with the org of the verified claim, never with an
//   org the caller supplied, and an artifact owned by another org is refused;
// * a typed rejection for a type outside the allow-list (Office documents,
//   images and archives are refused), for a file with no text layer, and for
//   text over Data Plane's content cap.
//
// The bytes of the file never travel: the caller hands over the text the parser
// already derived from them (`quarry-transform`), and the artifact id, type,
// size and digest ride in metadata for provenance.
// ---------------------------------------------------------------------------

/// The flag that turns the download hand-off on. Default off.
pub const ENV_DOWNLOAD_HANDOFF: &str = "QUARRY_EDGE__DOWNLOAD_HANDOFF";

/// Data Plane's `MaxContentLength` (documents-api `internal/validate`): the most
/// text one document may carry, in bytes.
pub const DOWNLOAD_HANDOFF_MAX_TEXT_BYTES: usize = 5 * 1024 * 1024;

/// The longest title the hand-off sends, in characters (documents-api allows
/// 1024; a file name is cut well below that).
const DOWNLOAD_TITLE_MAX_CHARS: usize = 255;

/// The metadata key that marks a Data Plane ingest as a download hand-off. The
/// wire mapping reads its `document_type` to pick the document type.
pub const DOWNLOAD_METADATA_KEY: &str = "quarry_download";

/// Whether a raw flag value turns the hand-off on. Unset or anything other than
/// the usual "on" spellings is off.
pub fn download_handoff_enabled(raw: Option<&str>) -> bool {
    crate::org_erasure::flag_on(raw)
}

/// Reads [`ENV_DOWNLOAD_HANDOFF`].
pub fn download_handoff_enabled_from_env() -> bool {
    download_handoff_enabled(std::env::var(ENV_DOWNLOAD_HANDOFF).ok().as_deref())
}

/// The download types the hand-off admits. Each is a type whose text the parser
/// derives and that Data Plane stores as a text document. Everything else
/// (Office documents, images, archives, executables) is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadDocumentKind {
    Pdf,
    PlainText,
    Markdown,
    Csv,
}

impl DownloadDocumentKind {
    pub const ALL: [Self; 4] = [Self::Pdf, Self::PlainText, Self::Markdown, Self::Csv];

    /// The kind for a MIME type. Parameters (`; charset=utf-8`) and case are
    /// ignored. `None` is a type outside the allow-list.
    pub fn from_mime(mime: &str) -> Option<Self> {
        match normalize_mime(mime).as_str() {
            "application/pdf" => Some(Self::Pdf),
            "text/plain" => Some(Self::PlainText),
            "text/markdown" | "text/x-markdown" => Some(Self::Markdown),
            "text/csv" => Some(Self::Csv),
            _ => None,
        }
    }

    /// The Data Plane document `type` this kind is stored as.
    pub fn document_type(self) -> &'static str {
        match self {
            Self::Pdf => "download_pdf",
            Self::PlainText => "download_text",
            Self::Markdown => "download_markdown",
            Self::Csv => "download_csv",
        }
    }

    /// The kind whose document type is `value`, if any.
    pub fn from_document_type(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.document_type() == value)
    }
}

/// The MIME type in lower case, without parameters.
fn normalize_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// Why a download was not handed off. Each is a typed, non-retryable refusal
/// with a stable `reason` in the error details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadHandoffRejection {
    /// `QUARRY_EDGE__DOWNLOAD_HANDOFF` is off.
    Disabled,
    /// The run is under ZDR: a durable Data Plane write is never allowed.
    ZdrRefused,
    /// The verified claim carries no org.
    MissingOrg,
    /// The artifact belongs to another org than the claim's.
    OrgMismatch,
    /// The type is outside the allow-list.
    UnsupportedType,
    /// The parser found no text (for example a scanned PDF without OCR).
    NoText,
    /// The text is over Data Plane's content cap.
    TooLarge,
    /// The download's source URL is not an http(s) URL.
    InvalidSourceUrl,
    /// The digest is not a 64-character hex SHA-256.
    InvalidDigest,
}

impl DownloadHandoffRejection {
    /// The stable reason written into the error details.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Disabled => "download_handoff_disabled",
            Self::ZdrRefused => "download_handoff_zdr_refused",
            Self::MissingOrg => "download_handoff_missing_org",
            Self::OrgMismatch => "download_handoff_org_mismatch",
            Self::UnsupportedType => "download_handoff_unsupported_type",
            Self::NoText => "download_handoff_no_text",
            Self::TooLarge => "download_handoff_too_large",
            Self::InvalidSourceUrl => "download_handoff_invalid_source_url",
            Self::InvalidDigest => "download_handoff_invalid_digest",
        }
    }

    /// The error class the refusal is reported as.
    pub fn code(self) -> ErrorCode {
        match self {
            Self::Disabled => ErrorCode::Unsupported,
            Self::ZdrRefused | Self::MissingOrg | Self::OrgMismatch => ErrorCode::Forbidden,
            Self::UnsupportedType | Self::NoText => ErrorCode::UnsupportedMedia,
            Self::TooLarge | Self::InvalidSourceUrl | Self::InvalidDigest => {
                ErrorCode::BadRequest
            }
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Disabled => "download hand-off to Data Plane is not enabled",
            Self::ZdrRefused => "ZDR active: download hand-off to Data Plane denied",
            Self::MissingOrg => "download hand-off needs the org of a verified claim",
            Self::OrgMismatch => "the downloaded artifact belongs to another org",
            Self::UnsupportedType => "this download type cannot be handed off to Data Plane",
            Self::NoText => "the download has no text to hand off",
            Self::TooLarge => "the download's text is over Data Plane's content cap",
            Self::InvalidSourceUrl => "the download's source URL is not an http(s) URL",
            Self::InvalidDigest => "the download's digest is not a SHA-256 hex digest",
        }
    }

    /// The typed error, with `details.reason`. Nothing caller-supplied is
    /// added, so the error is safe to log and return under any posture.
    pub fn into_error(self) -> QuarryError {
        let mut error = QuarryError::new(self.code(), self.message())
            .with_details(serde_json::json!({ "reason": self.reason() }));
        error.retryable = false;
        error
    }

    /// The typed error with one extra, already-safe detail field.
    fn into_error_with(self, key: &str, value: serde_json::Value) -> QuarryError {
        let mut details = serde_json::Map::new();
        details.insert("reason".to_string(), serde_json::Value::from(self.reason()));
        details.insert(key.to_string(), value);
        let mut error = QuarryError::new(self.code(), self.message())
            .with_details(serde_json::Value::Object(details));
        error.retryable = false;
        error
    }
}

/// The identity of the verified caller. The edge fills it from the claim it
/// already verified; the hand-off never reads an org from anywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadHandoffClaim {
    pub org_id: String,
    /// The initiating user, when the claim names one. Forwarded so Data Plane
    /// stamps ownership the same way it does for a page the user crawled.
    pub user_id: Option<String>,
}

/// One downloaded file, as the download path left it: a tenant-owned artifact
/// plus the text the parser derived from its bytes.
#[derive(Debug, Clone)]
pub struct DownloadedArtifact {
    pub artifact_id: quarry_core::ids::kinds::ArtifactKind,
    /// The org that owns the artifact row.
    pub org_id: String,
    pub run_id: quarry_core::ids::kinds::RunKind,
    /// The URL the file was downloaded from.
    pub source_url: String,
    /// The MIME type the download admission recorded.
    pub mime_type: String,
    pub byte_size: u64,
    /// SHA-256 of the bytes, lower-case hex.
    pub sha256: String,
    pub filename: Option<String>,
    /// The text the parser derived from the bytes.
    pub extracted_text: String,
    pub zdr: ZdrMode,
    pub privacy_policy: Option<PrivacyPolicy>,
}

/// Builds the Data Plane ingest request for one download, or the typed
/// refusal. Pure: the flag is checked by [`DownloadHandoff`].
///
/// Order matters: ZDR is refused before any other field is read, and the org
/// check comes before anything about the file, so a cross-tenant call learns
/// nothing about the artifact.
pub fn build_download_ingest_request(
    claim: &DownloadHandoffClaim,
    artifact: &DownloadedArtifact,
) -> QuarryResult<DataPlaneIngestRequest> {
    if artifact.zdr.is_active() {
        return Err(DownloadHandoffRejection::ZdrRefused.into_error());
    }
    let org_id = claim.org_id.trim();
    if org_id.is_empty() {
        return Err(DownloadHandoffRejection::MissingOrg.into_error());
    }
    if artifact.org_id != org_id {
        return Err(DownloadHandoffRejection::OrgMismatch.into_error());
    }
    let Some(kind) = DownloadDocumentKind::from_mime(&artifact.mime_type) else {
        let mime: String = normalize_mime(&artifact.mime_type).chars().take(128).collect();
        return Err(DownloadHandoffRejection::UnsupportedType
            .into_error_with("mime_type", serde_json::Value::String(mime)));
    };
    if artifact.extracted_text.trim().is_empty() {
        return Err(DownloadHandoffRejection::NoText.into_error());
    }
    if artifact.extracted_text.len() > DOWNLOAD_HANDOFF_MAX_TEXT_BYTES {
        return Err(DownloadHandoffRejection::TooLarge.into_error_with(
            "max_bytes",
            serde_json::Value::from(DOWNLOAD_HANDOFF_MAX_TEXT_BYTES as u64),
        ));
    }
    match url::Url::parse(&artifact.source_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => {}
        _ => return Err(DownloadHandoffRejection::InvalidSourceUrl.into_error()),
    }
    let sha256 = artifact.sha256.trim().to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(DownloadHandoffRejection::InvalidDigest.into_error());
    }

    let filename = artifact
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(DOWNLOAD_TITLE_MAX_CHARS).collect::<String>());

    let mut download = serde_json::Map::new();
    download.insert(
        "artifact_id".to_string(),
        serde_json::Value::String(artifact.artifact_id.to_string()),
    );
    download.insert(
        "document_type".to_string(),
        serde_json::Value::from(kind.document_type()),
    );
    download.insert(
        "mime_type".to_string(),
        serde_json::Value::String(normalize_mime(&artifact.mime_type)),
    );
    download.insert(
        "byte_size".to_string(),
        serde_json::Value::from(artifact.byte_size),
    );
    download.insert(
        "sha256".to_string(),
        serde_json::Value::String(sha256.clone()),
    );
    if let Some(name) = &filename {
        download.insert("filename".to_string(), serde_json::Value::String(name.clone()));
    }
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        DOWNLOAD_METADATA_KEY.to_string(),
        serde_json::Value::Object(download),
    );

    Ok(DataPlaneIngestRequest {
        run_id: artifact.run_id.clone(),
        // Stamped from the verified claim, never from the artifact row.
        org_id: org_id.to_string(),
        source_url: artifact.source_url.clone(),
        title: filename,
        markdown: Some(artifact.extracted_text.clone()),
        html_ref: None,
        raw_ref: None,
        chunks: Vec::new(),
        metadata: serde_json::Value::Object(metadata),
        fingerprint: format!("sha256:{sha256}"),
        zdr: artifact.zdr,
        retention_policy: None,
        privacy_policy: artifact.privacy_policy.clone(),
        source_trace: None,
        initiator_user_id: claim
            .user_id
            .as_deref()
            .map(str::trim)
            .filter(|user| !user.is_empty())
            .map(str::to_string),
        visibility: None,
    })
}

/// The governed hand-off: the flag, then [`build_download_ingest_request`],
/// then the one Data Plane ingest transport. The transport repeats its own ZDR
/// guard before any wire I/O.
pub struct DownloadHandoff {
    ingest: Arc<dyn DataPlaneIngest>,
    enabled: bool,
}

impl DownloadHandoff {
    pub fn new(ingest: Arc<dyn DataPlaneIngest>, enabled: bool) -> Self {
        Self { ingest, enabled }
    }

    /// Enabled by [`ENV_DOWNLOAD_HANDOFF`].
    pub fn from_env(ingest: Arc<dyn DataPlaneIngest>) -> Self {
        Self::new(ingest, download_handoff_enabled_from_env())
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Hands one download to Data Plane, or refuses it with a typed error.
    pub async fn hand_off(
        &self,
        claim: &DownloadHandoffClaim,
        artifact: &DownloadedArtifact,
    ) -> QuarryResult<DataPlaneIngestResponse> {
        if !self.enabled {
            return Err(DownloadHandoffRejection::Disabled.into_error());
        }
        let request = build_download_ingest_request(claim, artifact)?;
        self.ingest.ingest(&request).await
    }
}

/// The Data Plane document type for an ingest request: a download hand-off
/// carries an allow-listed type in its metadata; everything else is a page.
fn document_type_for(request: &DataPlaneIngestRequest) -> &'static str {
    request
        .metadata
        .get(DOWNLOAD_METADATA_KEY)
        .and_then(|download| download.get("document_type"))
        .and_then(|value| value.as_str())
        .and_then(DownloadDocumentKind::from_document_type)
        .map(DownloadDocumentKind::document_type)
        .unwrap_or("web_page")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_tokens::{ServiceBearer, ServiceTokenProvider, ServiceTokenRequest};
    use async_trait::async_trait;
    use quarry_core::contracts::{EmbeddingStatus, IndexStatus};
    use quarry_core::ids::Id;
    use quarry_core::zdr::ZdrMode;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_BEARER: &str =
        "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJzZXJ2aWNlOnF1YXJyeSJ9.c2lnbmF0dXJl";

    struct RotatingProvider {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ServiceTokenProvider for RotatingProvider {
        async fn token_for_org(
            &self,
            _org_id: &str,
            _request: &ServiceTokenRequest,
            force_refresh: bool,
        ) -> QuarryResult<ServiceBearer> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(force_refresh, call > 0);
            let token = if call == 0 {
                TEST_BEARER
            } else {
                "refreshed.token.signature"
            };
            Ok(ServiceBearer::from_test_token(token))
        }

        async fn invalidate(&self, _org_id: &str, _request: &ServiceTokenRequest) {}
    }

    #[derive(Clone)]
    struct RejectThenAccept(Arc<AtomicUsize>);

    impl wiremock::Respond for RejectThenAccept {
        fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
            let attempt = self.0.fetch_add(1, Ordering::SeqCst);
            let authorization = request
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap();
            if attempt == 0 {
                assert_eq!(authorization, format!("Bearer {TEST_BEARER}"));
                ResponseTemplate::new(401)
            } else {
                assert_eq!(authorization, "Bearer refreshed.token.signature");
                ResponseTemplate::new(201).set_body_json(json!({
                    "document_id": "doc_refreshed",
                    "status": "pending"
                }))
            }
        }
    }

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
            .and(header("authorization", format!("Bearer {TEST_BEARER}")))
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

        let client = IngestClient::new(server.uri(), TEST_BEARER).unwrap();
        let resp = client.ingest(&make_request()).await.unwrap();
        assert_eq!(resp.document_id, "doc_1");
        // Indexing + embedding run async downstream of the document write.
        assert_eq!(resp.index_status, IndexStatus::Pending);
        assert_eq!(resp.embedding_status, EmbeddingStatus::Pending);
        assert_eq!(resp.knowledge_unit_count, 0);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let headers = &requests[0].headers;
        assert!(!headers.contains_key("x-internal-api-key"));
        assert!(!headers.contains_key("x-org-id"));
        assert!(!headers.contains_key("x-user-id"));
    }

    #[tokio::test]
    async fn dynamic_ingest_refreshes_once_after_401() {
        let server = MockServer::start().await;
        let requests = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(RejectThenAccept(requests.clone()))
            .expect(2)
            .mount(&server)
            .await;
        let provider = Arc::new(RotatingProvider {
            calls: AtomicUsize::new(0),
        });
        let client = IngestClient::with_token_provider(server.uri(), provider.clone()).unwrap();

        let response = client.ingest(&make_request()).await.unwrap();

        assert_eq!(response.document_id, "doc_refreshed");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ingest_rate_limited() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), TEST_BEARER).unwrap();
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

        let client = IngestClient::new(server.uri(), TEST_BEARER).unwrap();
        let err = client.ingest(&make_request()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::DriverFailed);
    }

    #[tokio::test]
    async fn ingest_blocks_zdr_with_markdown_payload() {
        let mut req = make_request();
        req.zdr = ZdrMode::On;

        let client = IngestClient::new("http://localhost:1", TEST_BEARER).unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("ZDR=on"));
    }

    #[tokio::test]
    async fn ingest_zdr_no_payload_rejects_durable_write() {
        // ZDR=on with no durable payload: the client must NOT POST to the
        // durable route (which mandates content). Pointing at a dead address
        // proves the rejection happens before any request can fire.
        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.html_ref = None;
        req.raw_ref = None;

        let client = IngestClient::new("http://127.0.0.1:1", TEST_BEARER).unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("ingest write denied"));
    }

    #[test]
    fn ingest_client_rejects_shared_key_shape() {
        let err = IngestClient::new("http://127.0.0.1:1", "shared-key")
            .err()
            .expect("legacy shared key must be rejected");
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("signed bearer"));
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

        let client = IngestClient::new("http://localhost:1", TEST_BEARER).unwrap();
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
        let client = IngestClient::new("http://localhost:1", TEST_BEARER).unwrap();
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
        let client = IngestClient::new("http://localhost:1", TEST_BEARER).unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[tokio::test]
    async fn ingest_zdr_safe_metadata_still_rejects_durable_route() {
        // Safe (non-content) metadata clears the content-smuggling pre-check,
        // but the durable endpoint itself remains forbidden under ZDR.
        let mut req = make_request();
        req.zdr = ZdrMode::On;
        req.markdown = None;
        req.metadata = json!({
            "title": "Safe metadata",
            "language": "en",
            "fetched_at": "2026-05-08T00:00:00Z",
        });
        let client = IngestClient::new("http://127.0.0.1:1", TEST_BEARER).unwrap();
        let err = client.ingest(&req).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(err.message.contains("ingest write denied"));
    }

    #[tokio::test]
    async fn ingest_403_returns_forbidden() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(403).set_body_string("zdr violation"))
            .mount(&server)
            .await;

        let client = IngestClient::new(server.uri(), TEST_BEARER).unwrap();
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

/// E3.4-8: the download hand-off, against a hermetic fake of the ingest
/// transport (and one wiremock run of the real HTTP mapping).
#[cfg(test)]
mod download_handoff_tests {
    use super::*;
    use quarry_core::ids::Id;
    use serde_json::json;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_BEARER: &str =
        "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJzZXJ2aWNlOnF1YXJyeSJ9.c2lnbmF0dXJl";
    const DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    /// Records every request it is handed; never touches the network.
    #[derive(Default)]
    struct FakeIngest {
        seen: Mutex<Vec<DataPlaneIngestRequest>>,
    }

    impl FakeIngest {
        fn seen(&self) -> Vec<DataPlaneIngestRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl DataPlaneIngest for FakeIngest {
        async fn ingest(
            &self,
            request: &DataPlaneIngestRequest,
        ) -> QuarryResult<DataPlaneIngestResponse> {
            // The real transports refuse ZDR before wire I/O; so does the fake.
            ensure_durable_ingest_allowed(request.zdr)?;
            self.seen.lock().unwrap().push(request.clone());
            Ok(DataPlaneIngestResponse {
                document_id: "doc_download".into(),
                index_status: IndexStatus::Pending,
                knowledge_unit_count: 0,
                embedding_status: EmbeddingStatus::Pending,
                retrievable_after: None,
                trace_id: String::new(),
            })
        }
    }

    fn claim() -> DownloadHandoffClaim {
        DownloadHandoffClaim {
            org_id: "org_a".into(),
            user_id: Some("user_1".into()),
        }
    }

    fn artifact() -> DownloadedArtifact {
        DownloadedArtifact {
            artifact_id: Id::new(),
            org_id: "org_a".into(),
            run_id: Id::new(),
            source_url: "https://example.com/files/report.pdf".into(),
            mime_type: "application/PDF; charset=binary".into(),
            byte_size: 4_096,
            sha256: DIGEST.into(),
            filename: Some("report.pdf".into()),
            extracted_text: "Quarterly report\n\nRevenue grew.".into(),
            zdr: ZdrMode::Off,
            privacy_policy: Some(PrivacyPolicy::default()),
        }
    }

    fn reason(error: &QuarryError) -> &str {
        error
            .details
            .as_ref()
            .and_then(|details| details.get("reason"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
    }

    #[test]
    fn the_flag_is_off_unless_turned_on() {
        assert!(!download_handoff_enabled(None));
        assert!(!download_handoff_enabled(Some("")));
        assert!(!download_handoff_enabled(Some("off")));
        assert!(!download_handoff_enabled(Some("observe")));
        assert!(download_handoff_enabled(Some("on")));
        assert!(download_handoff_enabled(Some(" TRUE ")));
        assert!(download_handoff_enabled(Some("1")));
        assert_eq!(ENV_DOWNLOAD_HANDOFF, "QUARRY_EDGE__DOWNLOAD_HANDOFF");
    }

    #[tokio::test]
    async fn a_disabled_handoff_refuses_without_calling_data_plane() {
        let fake = Arc::new(FakeIngest::default());
        let handoff = DownloadHandoff::new(fake.clone(), false);
        assert!(!handoff.enabled());
        let error = handoff.hand_off(&claim(), &artifact()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(reason(&error), "download_handoff_disabled");
        assert!(!error.retryable);
        assert!(fake.seen().is_empty());
    }

    #[tokio::test]
    async fn an_admitted_pdf_is_stamped_with_the_claims_org_and_handed_off() {
        let fake = Arc::new(FakeIngest::default());
        let handoff = DownloadHandoff::new(fake.clone(), true);
        let artifact = artifact();
        let response = handoff.hand_off(&claim(), &artifact).await.unwrap();
        assert_eq!(response.document_id, "doc_download");

        let seen = fake.seen();
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert_eq!(request.org_id, "org_a");
        assert_eq!(request.initiator_user_id.as_deref(), Some("user_1"));
        assert_eq!(request.source_url, artifact.source_url);
        assert_eq!(request.title.as_deref(), Some("report.pdf"));
        assert_eq!(request.markdown.as_deref(), Some(artifact.extracted_text.as_str()));
        assert_eq!(request.fingerprint, format!("sha256:{DIGEST}"));
        assert!(request.html_ref.is_none() && request.raw_ref.is_none());
        assert!(request.chunks.is_empty());
        let download = &request.metadata[DOWNLOAD_METADATA_KEY];
        assert_eq!(download["document_type"], "download_pdf");
        assert_eq!(download["mime_type"], "application/pdf");
        assert_eq!(download["byte_size"], 4_096);
        assert_eq!(download["sha256"], DIGEST);
        assert_eq!(download["artifact_id"], artifact.artifact_id.to_string());
    }

    #[tokio::test]
    async fn zdr_is_refused_before_anything_else() {
        let fake = Arc::new(FakeIngest::default());
        let handoff = DownloadHandoff::new(fake.clone(), true);
        let mut artifact = artifact();
        artifact.zdr = ZdrMode::On;
        // Also wrong org and wrong type: ZDR still wins, and says nothing else.
        artifact.org_id = "org_b".into();
        artifact.mime_type = "application/zip".into();
        let error = handoff.hand_off(&claim(), &artifact).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(reason(&error), "download_handoff_zdr_refused");
        assert_eq!(error.details, Some(json!({ "reason": "download_handoff_zdr_refused" })));
        assert!(fake.seen().is_empty());
    }

    #[tokio::test]
    async fn an_artifact_of_another_org_is_refused() {
        let fake = Arc::new(FakeIngest::default());
        let handoff = DownloadHandoff::new(fake.clone(), true);
        let mut artifact = artifact();
        artifact.org_id = "org_b".into();
        let error = handoff.hand_off(&claim(), &artifact).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(reason(&error), "download_handoff_org_mismatch");
        assert!(fake.seen().is_empty());
    }

    #[test]
    fn a_claim_without_an_org_is_refused() {
        let claim = DownloadHandoffClaim {
            org_id: "  ".into(),
            user_id: None,
        };
        let error = build_download_ingest_request(&claim, &artifact()).unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(reason(&error), "download_handoff_missing_org");
    }

    #[test]
    fn types_outside_the_allow_list_get_a_typed_rejection() {
        for mime in [
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/msword",
            "application/vnd.ms-excel",
            "image/png",
            "application/zip",
            "application/octet-stream",
            "text/html",
            "",
        ] {
            let mut artifact = artifact();
            artifact.mime_type = mime.into();
            let error = build_download_ingest_request(&claim(), &artifact).unwrap_err();
            assert_eq!(error.code, ErrorCode::UnsupportedMedia, "{mime}");
            assert_eq!(reason(&error), "download_handoff_unsupported_type", "{mime}");
            assert!(!error.retryable);
        }
    }

    #[test]
    fn each_admitted_type_maps_to_its_document_type() {
        let cases = [
            ("application/pdf", "download_pdf"),
            ("text/plain; charset=utf-8", "download_text"),
            ("text/markdown", "download_markdown"),
            ("text/csv", "download_csv"),
        ];
        for (mime, document_type) in cases {
            let mut artifact = artifact();
            artifact.mime_type = mime.into();
            let request = build_download_ingest_request(&claim(), &artifact).unwrap();
            assert_eq!(document_type_for(&request), document_type, "{mime}");
        }
    }

    #[test]
    fn a_download_without_text_is_refused() {
        let mut artifact = artifact();
        artifact.extracted_text = " \n\t ".into();
        let error = build_download_ingest_request(&claim(), &artifact).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsupportedMedia);
        assert_eq!(reason(&error), "download_handoff_no_text");
    }

    #[test]
    fn text_over_the_data_plane_cap_is_refused() {
        let mut artifact = artifact();
        artifact.extracted_text = "x".repeat(DOWNLOAD_HANDOFF_MAX_TEXT_BYTES + 1);
        let error = build_download_ingest_request(&claim(), &artifact).unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert_eq!(reason(&error), "download_handoff_too_large");
    }

    #[test]
    fn a_non_http_source_and_a_bad_digest_are_refused() {
        let mut artifact = artifact();
        artifact.source_url = "file:///etc/passwd".into();
        let error = build_download_ingest_request(&claim(), &artifact).unwrap_err();
        assert_eq!(reason(&error), "download_handoff_invalid_source_url");

        let mut artifact = self::artifact();
        artifact.sha256 = "not-a-digest".into();
        let error = build_download_ingest_request(&claim(), &artifact).unwrap_err();
        assert_eq!(reason(&error), "download_handoff_invalid_digest");
    }

    #[test]
    fn a_page_ingest_keeps_the_web_page_type() {
        let mut artifact = artifact();
        artifact.filename = None;
        let mut request = build_download_ingest_request(&claim(), &artifact).unwrap();
        // An unknown document type in the marker falls back to a page.
        request.metadata[DOWNLOAD_METADATA_KEY]["document_type"] = json!("download_exe");
        assert_eq!(document_type_for(&request), "web_page");
        request.metadata = json!({});
        assert_eq!(document_type_for(&request), "web_page");
    }

    #[tokio::test]
    async fn the_http_transport_sends_the_download_as_a_typed_document() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/documents"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "document_id": "doc_pdf",
                "status": "pending"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = IngestClient::new(server.uri(), TEST_BEARER).unwrap();
        let handoff = DownloadHandoff::new(Arc::new(client), true);

        let response = handoff.hand_off(&claim(), &artifact()).await.unwrap();
        assert_eq!(response.document_id, "doc_pdf");

        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["org_id"], "org_a");
        assert_eq!(body["source"], "quarry");
        assert_eq!(body["type"], "download_pdf");
        assert_eq!(body["title"], "report.pdf");
        assert_eq!(body["content"], "Quarterly report\n\nRevenue grew.");
        assert_eq!(body["metadata"]["url"], "https://example.com/files/report.pdf");
        assert_eq!(body["metadata"][DOWNLOAD_METADATA_KEY]["document_type"], "download_pdf");
        assert_eq!(body["metadata"]["fingerprint"], format!("sha256:{DIGEST}"));
    }

    #[tokio::test]
    async fn the_http_transport_never_sends_a_zdr_download() {
        // Dead address: a refusal must come before any request could fire.
        let client = IngestClient::new("http://127.0.0.1:1", TEST_BEARER).unwrap();
        let handoff = DownloadHandoff::new(Arc::new(client), true);
        let mut artifact = artifact();
        artifact.zdr = ZdrMode::On;
        let error = handoff.hand_off(&claim(), &artifact).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(reason(&error), "download_handoff_zdr_refused");
    }
}
